//! T137 — cancelling leave (FR-018, FR-020, FR-046, FR-059).
//!
//! The clock starts on Tue 24 Nov 2026, 14:30 in Asia/Kolkata. Asha is in Eng (approver Priya);
//! Vik approves Sales; Ravi is kept informed of Asha's requests.
mod common;

use chrono::{TimeZone, Utc};
use common::factories::*;
use common::*;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use serde_json::{json, Value};
use uuid::Uuid;

struct World {
    app: TestApp,
    admin: reqwest::Client,
    priya: reqwest::Client,
    vik: reqwest::Client,
    asha_user: User,
    asha: reqwest::Client,
    ravi: reqwest::Client,
    cl: Uuid,
}

async fn long_session(app: &TestApp, user: &User) -> reqwest::Client {
    app.with_token(&create_session(app, user, true).await)
}

async fn world() -> World {
    let app = TestApp::spawn().await;
    let (org, admin_user, _) = org_with_admin(&app, "acme.test").await;
    let cl = create_leave_type(&app, &org, NewLeaveType::all("CL", dec!(12))).await;
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
    add_ledger_row(&app, &asha_user, cl, 2026, dec!(12), "allotment").await;
    // "Keep me signed in" sessions survive the clock moving forward a week or two.
    World {
        admin: long_session(&app, &admin_user).await,
        priya: long_session(&app, &priya_u).await,
        vik: long_session(&app, &vik_u).await,
        asha: long_session(&app, &asha_user).await,
        ravi: long_session(&app, &ravi_u).await,
        asha_user,
        cl,
        app,
    }
}

/// Asha requests `from..=to` with Ravi kept informed.
async fn request(w: &World, from: &str, to: &str) -> String {
    let mut body = leave_body(w.cl, from, to);
    body["notify"] = json!([{ "email": "ravi@acme.test" }]);
    submit(&w.app, &w.asha, &body).await
}

async fn approved(w: &World, from: &str, to: &str) -> String {
    let id = request(w, from, to).await;
    let (status, out) = decide(&w.app, &w.priya, &id, approve()).await;
    assert_eq!(status, 200, "{out}");
    id
}

async fn cancel(w: &World, client: &reqwest::Client, id: &str, body: Value) -> (u16, Value) {
    post_json(
        client,
        w.app.url(&format!("/api/v1/leaves/{id}/cancel")),
        body,
    )
    .await
}

async fn decide_cancel(w: &World, client: &reqwest::Client, id: &str, body: Value) -> (u16, Value) {
    let res = client
        .post(w.app.url(&format!("/api/v1/approvals/cancel/{id}/decide")))
        .header("Idempotency-Key", key())
        .json(&body)
        .send()
        .await
        .unwrap();
    let status = res.status().as_u16();
    (status, res.json().await.unwrap_or(Value::Null))
}

async fn card(w: &World) -> (Value, Value, Value) {
    let c = balance_card(&w.app, &w.asha, "CL", None).await;
    (
        c["taken"].clone(),
        c["pending"].clone(),
        c["available"].clone(),
    )
}

async fn ledger(w: &World) -> Vec<(String, Decimal)> {
    ledger_rows(&w.app, w.asha_user.id, w.cl)
        .await
        .into_iter()
        .map(|(reason, delta, _, _)| (reason, delta))
        .collect()
}

async fn active_days(w: &World, id: &str) -> Vec<String> {
    sqlx::query_scalar(
        "SELECT day::text FROM leave_request_days
         WHERE leave_request_id = $1::uuid AND active ORDER BY day",
    )
    .bind(id)
    .fetch_all(&w.app.db.owner)
    .await
    .unwrap()
}

/// Emails sent since the last call, as `(to, subject, text)`.
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

/// Everyone involved hears about the cancellation; the FYI copy is minimal (FR-046, FR-059).
fn assert_cancel_emails(sent: &[(String, String, String)]) {
    let mine = to(sent, "asha@acme.test");
    assert!(mine.1.to_lowercase().contains("cancelled"), "{}", mine.1);
    assert!(mine.1.contains("Casual Leave"), "{}", mine.1);
    for approver in ["priya@acme.test", "admin@acme.test"] {
        let m = to(sent, approver);
        assert!(m.1.to_lowercase().contains("cancelled"), "{}", m.1);
        assert!(m.1.contains("Asha Rao"), "{}", m.1);
    }
    let fyi = to(sent, "ravi@acme.test");
    assert!(fyi.2.to_lowercase().contains("cancelled"), "{}", fyi.2);
    for part in [&fyi.1, &fyi.2] {
        assert!(!part.contains("Casual"), "{part}");
        assert!(!part.contains("Family function"), "{part}");
    }
    assert!(
        !sent.iter().any(|m| m.0 == "vik@acme.test"),
        "Vik is not Asha's approver"
    );
}

#[tokio::test]
async fn withdrawing_a_pending_request_releases_its_days() {
    let w = world().await;
    let id = request(&w, "2026-11-30", "2026-12-02").await;
    assert_eq!(card(&w).await, (json!(0.0), json!(3.0), json!(9.0)));
    mail(&w).await;

    let (status, out) = cancel(&w, &w.asha, &id, json!({})).await;
    assert_eq!(status, 200, "{out}");
    assert_eq!(out["status"], "cancelled");
    assert_eq!(card(&w).await, (json!(0.0), json!(0.0), json!(12.0)));
    assert_eq!(ledger(&w).await, [("allotment".to_string(), dec!(12))]);
    assert!(active_days(&w, &id).await.is_empty());
    let (_, detail) = get_json(&w.asha, w.app.url(&format!("/api/v1/leaves/{id}"))).await;
    assert_eq!(detail["status"], "cancelled");
    // It has left the inbox, and the same dates can be requested again.
    let (_, inbox) = get_json(&w.priya, w.app.url("/api/v1/approvals")).await;
    assert_eq!(inbox, json!([]));
    assert_cancel_emails(&mail(&w).await);
    request(&w, "2026-11-30", "2026-12-02").await;
}

#[tokio::test]
async fn cancelling_approved_leave_before_it_starts_returns_every_day() {
    let w = world().await;
    let id = approved(&w, "2026-11-30", "2026-12-02").await;
    assert_eq!(card(&w).await, (json!(3.0), json!(0.0), json!(9.0)));
    mail(&w).await;

    // The day before it starts (Sun 29 Nov, 23:30 in Kolkata).
    w.app
        .clock
        .set(Utc.with_ymd_and_hms(2026, 11, 29, 18, 0, 0).unwrap());
    let (status, out) = cancel(&w, &w.asha, &id, json!({ "reason": "Plans changed" })).await;
    assert_eq!(status, 200, "{out}");
    assert_eq!(out["status"], "cancelled");
    assert_eq!(card(&w).await, (json!(0.0), json!(0.0), json!(12.0)));
    assert_eq!(
        ledger(&w).await,
        [
            ("allotment".to_string(), dec!(12)),
            ("leave_taken".to_string(), dec!(-3)),
            ("leave_reversed".to_string(), dec!(3)),
        ]
    );
    assert!(active_days(&w, &id).await.is_empty());
    let (_, away) = get_json(&w.asha, w.app.url("/api/v1/away?range=month")).await;
    assert_eq!(away["items"], json!([]));
    assert_cancel_emails(&mail(&w).await);
}

/// A 5-day leave Mon 30 Nov – Fri 4 Dec; on Wed 2 Dec Asha asks to cancel the rest.
async fn started(w: &World) -> String {
    let id = approved(w, "2026-11-30", "2026-12-04").await;
    mail(w).await;
    w.app
        .clock
        .set(Utc.with_ymd_and_hms(2026, 12, 2, 4, 30, 0).unwrap());
    let (status, out) = cancel(w, &w.asha, &id, json!({ "reason": "Back early" })).await;
    assert_eq!(status, 200, "{out}");
    assert_eq!(out["status"], "cancel_requested");
    id
}

#[tokio::test]
async fn cancelling_started_leave_waits_for_an_approver() {
    let w = world().await;
    let id = started(&w).await;
    // Nothing is returned yet.
    assert_eq!(card(&w).await, (json!(5.0), json!(0.0), json!(7.0)));
    assert_eq!(active_days(&w, &id).await.len(), 5);
    let (_, detail) = get_json(&w.asha, w.app.url(&format!("/api/v1/leaves/{id}"))).await;
    assert_eq!(detail["status"], "cancel_requested");
    assert_eq!(detail["version"], 3);

    let (status, inbox) = get_json(&w.priya, w.app.url("/api/v1/approvals?kind=cancel")).await;
    assert_eq!(status, 200, "{inbox}");
    let items = inbox.as_array().unwrap();
    assert_eq!(items.len(), 1, "{inbox}");
    let item = &items[0];
    assert_eq!(item["id"], id);
    assert_eq!(item["kind"], "cancel");
    assert_eq!(item["requester"]["full_name"], "Asha Rao");
    assert_eq!(item["days"], 5.0);
    assert_eq!(item["version"], 3);
    assert_eq!(item["cancel"]["reason"], "Back early");
    assert_eq!(item["cancel"]["requested_at"], "2026-12-02T04:30:00Z");
    assert_eq!(item["cancel"]["days_returned"], 3.0);
    // Not a leave decision, and not for other groups' approvers.
    let (_, leave_inbox) = get_json(&w.priya, w.app.url("/api/v1/approvals")).await;
    assert_eq!(leave_inbox, json!([]));
    let (_, vik_inbox) = get_json(&w.vik, w.app.url("/api/v1/approvals?kind=cancel")).await;
    assert_eq!(vik_inbox, json!([]));
    let (_, admin_inbox) = get_json(&w.admin, w.app.url("/api/v1/approvals?kind=cancel")).await;
    assert_eq!(admin_inbox.as_array().unwrap().len(), 1);

    // Approvers are asked to act; the employee is told it is waiting.
    let sent = mail(&w).await;
    for approver in ["priya@acme.test", "admin@acme.test"] {
        let m = to(&sent, approver);
        assert!(m.1.contains("Asha Rao"), "{}", m.1);
        assert!(m.1.to_lowercase().contains("cancel"), "{}", m.1);
        assert!(m.2.contains("Back early"), "{}", m.2);
        assert!(m.2.contains("/approvals"), "{}", m.2);
    }
    assert!(!sent.iter().any(|m| m.0 == "vik@acme.test"));
}

#[tokio::test]
async fn accepting_returns_only_days_from_the_request_date() {
    let w = world().await;
    let id = started(&w).await;
    mail(&w).await;
    // Accepted two days later: the cut-off is still the request date, Wed 2 Dec.
    w.app
        .clock
        .set(Utc.with_ymd_and_hms(2026, 12, 4, 4, 30, 0).unwrap());
    let (status, out) = decide_cancel(
        &w,
        &w.priya,
        &id,
        json!({ "decision": "approve", "version": 3 }),
    )
    .await;
    assert_eq!(status, 200, "{out}");
    assert_eq!(out["status"], "cancelled");
    assert_eq!(out["decided_by"]["full_name"], "Priya Shah");

    assert_eq!(card(&w).await, (json!(2.0), json!(0.0), json!(10.0)));
    assert_eq!(
        ledger(&w).await,
        [
            ("allotment".to_string(), dec!(12)),
            ("leave_taken".to_string(), dec!(-5)),
            ("leave_reversed".to_string(), dec!(3)),
        ]
    );
    assert_eq!(active_days(&w, &id).await, ["2026-11-30", "2026-12-01"]);
    let (_, detail) = get_json(&w.asha, w.app.url(&format!("/api/v1/leaves/{id}"))).await;
    assert_eq!(detail["status"], "cancelled");
    let (_, inbox) = get_json(&w.priya, w.app.url("/api/v1/approvals?kind=cancel")).await;
    assert_eq!(inbox, json!([]));
    assert_cancel_emails(&mail(&w).await);

    // Deciding again loses the race.
    let (status, out) = decide_cancel(
        &w,
        &w.admin,
        &id,
        json!({ "decision": "approve", "version": 3 }),
    )
    .await;
    assert_eq!(status, 409, "{out}");
    assert_eq!(out["error"]["code"], "ALREADY_DECIDED");
}

#[tokio::test]
async fn declining_keeps_the_leave_approved() {
    let w = world().await;
    let id = started(&w).await;
    mail(&w).await;
    let (status, out) = decide_cancel(
        &w,
        &w.priya,
        &id,
        json!({ "decision": "reject", "version": 3 }),
    )
    .await;
    assert_eq!(status, 422, "{out}");
    assert_eq!(out["error"]["code"], "COMMENT_REQUIRED");

    let (status, out) = decide_cancel(
        &w,
        &w.priya,
        &id,
        json!({ "decision": "reject", "version": 3, "comment": "Release is this week" }),
    )
    .await;
    assert_eq!(status, 200, "{out}");
    assert_eq!(out["status"], "approved");
    assert_eq!(card(&w).await, (json!(5.0), json!(0.0), json!(7.0)));
    assert_eq!(active_days(&w, &id).await.len(), 5);
    let (_, detail) = get_json(&w.asha, w.app.url(&format!("/api/v1/leaves/{id}"))).await;
    assert_eq!(detail["status"], "approved");

    let sent = mail(&w).await;
    let mine = to(&sent, "asha@acme.test");
    assert!(mine.1.to_lowercase().contains("declined"), "{}", mine.1);
    assert!(mine.2.contains("Release is this week"), "{}", mine.2);
}

#[tokio::test]
async fn only_the_owner_cancels_and_only_once() {
    let w = world().await;
    let id = request(&w, "2026-11-30", "2026-12-02").await;
    // An eligible approver can see it but not cancel it; others cannot see it at all.
    let (status, out) = cancel(&w, &w.priya, &id, json!({})).await;
    assert_eq!(status, 403, "{out}");
    assert_eq!(out["error"]["code"], "FORBIDDEN");
    let (status, _) = cancel(&w, &w.ravi, &id, json!({})).await;
    assert_eq!(status, 404);
    let (status, _) = cancel(&w, &w.app.anon(), &id, json!({})).await;
    assert_eq!(status, 401);
    let (status, out) = cancel(&w, &w.asha, &id, json!({ "reason": "x".repeat(501) })).await;
    assert_eq!(status, 422, "{out}");
    assert_eq!(out["error"]["details"]["field"], "reason");

    let (status, _) = cancel(&w, &w.asha, &id, json!({})).await;
    assert_eq!(status, 200);
    let (status, out) = cancel(&w, &w.asha, &id, json!({})).await;
    assert_eq!(status, 409, "{out}");
    assert_eq!(out["error"]["code"], "ALREADY_DECIDED");
    assert_eq!(out["error"]["details"]["status"], "cancelled");

    // A rejected request cannot be cancelled either.
    let other = request(&w, "2026-12-07", "2026-12-07").await;
    let (status, _) = decide(
        &w.app,
        &w.priya,
        &other,
        json!({ "decision": "reject", "version": 1, "comment": "No" }),
    )
    .await;
    assert_eq!(status, 200);
    let (status, _) = cancel(&w, &w.asha, &other, json!({})).await;
    assert_eq!(status, 409);
}

#[tokio::test]
async fn cancellation_and_leave_decisions_do_not_mix() {
    let w = world().await;
    let pending = request(&w, "2026-12-07", "2026-12-07").await;
    // A pending leave is not a cancellation to decide.
    let (status, _) = decide_cancel(
        &w,
        &w.priya,
        &pending,
        json!({ "decision": "approve", "version": 1 }),
    )
    .await;
    assert_eq!(status, 409);
    // A cancellation request is not a leave to approve.
    let id = started(&w).await;
    let (status, _) = decide(
        &w.app,
        &w.priya,
        &id,
        json!({ "decision": "approve", "version": 3 }),
    )
    .await;
    assert_eq!(status, 409);
    // The requester cannot accept their own cancellation; other groups' approvers cannot either.
    let (status, out) = decide_cancel(
        &w,
        &w.asha,
        &id,
        json!({ "decision": "approve", "version": 3 }),
    )
    .await;
    assert_eq!(status, 403, "{out}");
    assert_eq!(out["error"]["code"], "SELF_APPROVAL");
    let (status, out) = decide_cancel(
        &w,
        &w.vik,
        &id,
        json!({ "decision": "approve", "version": 3 }),
    )
    .await;
    assert_eq!(status, 403, "{out}");
    assert_eq!(out["error"]["code"], "NOT_ELIGIBLE_APPROVER");
}
