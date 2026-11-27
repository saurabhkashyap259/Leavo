//! T093 — `POST /leaves` (FR-017–FR-019, FR-022, FR-055, FR-059).
mod common;

use common::factories::*;
use common::*;
use rust_decimal_macros::dec;
use serde_json::json;
use uuid::Uuid;

struct Fixture {
    app: TestApp,
    org: Org,
    admin: User,
    priya: User,
    vik: User,
    ravi: User,
    asha: User,
    client: reqwest::Client,
    cl: Uuid,
    eng: Uuid,
}

/// Asha is in the Eng group (approver Priya); Vik approves another group; Ravi is a colleague.
async fn fixture() -> Fixture {
    let app = TestApp::spawn().await;
    let (org, admin, _) = org_with_admin(&app, "acme.test").await;
    let cl = create_leave_type(&app, &org, NewLeaveType::all("CL", dec!(12))).await;
    sqlx::query("UPDATE leave_types SET name = 'Casual Leave' WHERE id = $1")
        .bind(cl)
        .execute(&app.db.owner)
        .await
        .unwrap();
    let priya = create_user(
        &app,
        &org,
        NewUser::employee("priya@acme.test").role("approver"),
    )
    .await;
    let vik = create_user(
        &app,
        &org,
        NewUser::employee("vik@acme.test").role("approver"),
    )
    .await;
    let eng = create_group(&app, &org, "Eng", &[&priya]).await;
    create_group(&app, &org, "Sales", &[&vik]).await;
    let ravi = create_user(&app, &org, NewUser::employee("ravi@acme.test")).await;
    let asha = create_user(&app, &org, NewUser::employee("asha@acme.test").group(eng)).await;
    add_ledger_row(&app, &asha, cl, 2026, dec!(12), "allotment").await;
    let client = app.login_as(&asha).await;
    Fixture {
        app,
        org,
        admin,
        priya,
        vik,
        ravi,
        asha,
        client,
        cl,
        eng,
    }
}

fn body_with_notify(f: &Fixture) -> serde_json::Value {
    let mut body = leave_body(f.cl, "2026-11-30", "2026-12-02");
    body["notify"] = json!([{ "user_id": f.ravi.id }, { "email": "Client@External.com" }]);
    body
}

#[tokio::test]
async fn create_reserves_pending_days_and_routes_to_the_group() {
    let f = fixture().await;
    let (status, out) = post_leave(&f.app, &f.client, &body_with_notify(&f), &key()).await;
    assert_eq!(status, 201, "{out}");
    assert_eq!(out["status"], "pending");
    assert_eq!(out["days"], 3.0);
    assert_eq!(
        out["by_leave_year"],
        json!([{ "leave_year": 2026, "days": 3.0, "available_after": 9.0 }])
    );

    let id: Uuid = out["id"].as_str().unwrap().parse().unwrap();
    let (routed, days): (Option<Uuid>, rust_decimal::Decimal) =
        sqlx::query_as("SELECT routed_group_id, days FROM leave_requests WHERE id = $1")
            .bind(id)
            .fetch_one(&f.app.db.owner)
            .await
            .unwrap();
    assert_eq!(routed, Some(f.eng));
    assert_eq!(days, dec!(3));
    let booked: Vec<(chrono::NaiveDate, i32)> = sqlx::query_as(
        "SELECT day, leave_year FROM leave_request_days WHERE leave_request_id = $1 ORDER BY day",
    )
    .bind(id)
    .fetch_all(&f.app.db.owner)
    .await
    .unwrap();
    assert_eq!(booked.len(), 3);

    // Pending, not taken: no ledger rows yet.
    let card = balance_card(&f.app, &f.client, "CL", None).await;
    assert_eq!(card["pending"], 3.0);
    assert_eq!(card["taken"], 0.0);
    assert_eq!(card["available"], 9.0);
    assert_eq!(ledger_rows(&f.app, f.asha.id, f.cl).await.len(), 1);

    // Notified members are stored, external emails lower-cased.
    let notified: Vec<(Option<Uuid>, String)> = sqlx::query_as(
        "SELECT user_id, email::text FROM request_notifications WHERE request_id = $1 ORDER BY email",
    )
    .bind(id)
    .fetch_all(&f.app.db.owner)
    .await
    .unwrap();
    assert_eq!(
        notified,
        [
            (None, "client@external.com".to_string()),
            (Some(f.ravi.id), "ravi@acme.test".to_string())
        ]
    );

    // GET /leaves/{id} shows the detail to the owner.
    let (status, detail) = get_json(&f.client, f.app.url(&format!("/api/v1/leaves/{id}"))).await;
    assert_eq!(status, 200, "{detail}");
    assert_eq!(detail["status"], "pending");
    assert_eq!(detail["reason"], "Family function");
    assert_eq!(detail["leave_type"]["code"], "CL");
    assert_eq!(detail["version"], 1);
}

#[tokio::test]
async fn emails_go_to_the_employee_the_eligible_approvers_and_the_notified_members() {
    let f = fixture().await;
    let (status, out) = post_leave(&f.app, &f.client, &body_with_notify(&f), &key()).await;
    assert_eq!(status, 201, "{out}");
    f.app.run_jobs().await;
    let sent = f.app.email.sent();
    let to = |addr: &str| sent.iter().filter(|m| m.to == addr).collect::<Vec<_>>();

    // Confirmation to the employee.
    let mine = to("asha@acme.test");
    assert_eq!(mine.len(), 1);
    assert!(mine[0].text.contains("Casual Leave"));

    // Action emails to the group's approver and the admin, never to other approvers.
    for addr in ["priya@acme.test", "admin@acme.test"] {
        let m = to(addr);
        assert_eq!(m.len(), 1, "{addr}");
        assert!(m[0].text.contains("Casual Leave"), "{addr}: {}", m[0].text);
        assert!(m[0].text.contains("Family function"), "{addr}");
        assert!(m[0].text.contains("/approvals"), "{addr}");
    }
    assert!(to("vik@acme.test").is_empty());

    // FYI: name, dates, days, status only (FR-059).
    for addr in ["ravi@acme.test", "client@external.com"] {
        let m = to(addr);
        assert_eq!(m.len(), 1, "{addr}");
        let fyi = m[0];
        for part in [&fyi.subject, &fyi.text, &fyi.html] {
            assert!(!part.contains("Casual"), "{addr} mentions the type: {part}");
            assert!(
                !part.contains("Family function"),
                "{addr} mentions the reason"
            );
            assert!(!part.contains("http"), "{addr} has a link: {part}");
            assert!(
                !part.contains("approv"),
                "{addr} talks about approving: {part}"
            );
        }
        assert!(fyi.text.contains("asha"), "{}", fyi.text);
        assert!(fyi.text.contains("30 Nov 2026"), "{}", fyi.text);
        assert!(fyi.text.contains("2 Dec 2026"), "{}", fyi.text);
        assert!(fyi.text.contains("3 days"), "{}", fyi.text);
        assert!(fyi.text.to_lowercase().contains("pending"), "{}", fyi.text);
        assert!(fyi.attachments.is_empty());
    }
    assert_eq!(sent.len(), 5);
    let _ = (&f.admin, &f.priya, &f.vik);
}

#[tokio::test]
async fn a_future_leave_year_uses_the_projected_quota() {
    let f = fixture().await;
    // Mon 4 – Tue 5 Jan 2027: leave year 2027 has not started; 12 projected, 2 pending.
    let (status, out) = post_leave(
        &f.app,
        &f.client,
        &leave_body(f.cl, "2027-01-04", "2027-01-05"),
        &key(),
    )
    .await;
    assert_eq!(status, 201, "{out}");
    assert_eq!(
        out["by_leave_year"],
        json!([{ "leave_year": 2027, "days": 2.0, "available_after": 10.0 }])
    );
    let card = balance_card(&f.app, &f.client, "CL", Some(2027)).await;
    assert_eq!(card["allotted"], 12.0);
    assert_eq!(card["pending"], 2.0);
    assert_eq!(card["available"], 10.0);
    // The current year is untouched.
    let card = balance_card(&f.app, &f.client, "CL", None).await;
    assert_eq!(card["pending"], 0.0);
    assert_eq!(card["available"], 12.0);

    // More than the projected quota is refused for that year.
    let (status, out) = post_leave(
        &f.app,
        &f.client,
        &leave_body(f.cl, "2027-01-06", "2027-01-21"),
        &key(),
    )
    .await;
    assert_eq!(status, 422, "{out}");
    assert_eq!(out["error"]["code"], "INSUFFICIENT_BALANCE");
    assert_eq!(out["error"]["details"]["leave_year"], 2027);
}

#[tokio::test]
async fn idempotency_key_replays_the_first_response() {
    let f = fixture().await;
    let k = key();
    let body = leave_body(f.cl, "2026-12-01", "2026-12-01");
    let (s1, first) = post_leave(&f.app, &f.client, &body, &k).await;
    let (s2, second) = post_leave(&f.app, &f.client, &body, &k).await;
    assert_eq!((s1, s2), (201, 201));
    assert_eq!(first, second);
    let (n,): (i64,) = sqlx::query_as("SELECT count(*) FROM leave_requests")
        .fetch_one(&f.app.db.owner)
        .await
        .unwrap();
    assert_eq!(n, 1);

    // Same key, different body.
    let (status, out) = post_leave(
        &f.app,
        &f.client,
        &leave_body(f.cl, "2026-12-03", "2026-12-03"),
        &k,
    )
    .await;
    assert_eq!(status, 409, "{out}");
    assert_eq!(out["error"]["code"], "IDEMPOTENCY_MISMATCH");

    // The key is required.
    let (status, out) = post_json(&f.client, f.app.url("/api/v1/leaves"), body).await;
    assert_eq!(status, 422, "{out}");
    assert_eq!(out["error"]["details"]["field"], "Idempotency-Key");
}

#[tokio::test]
async fn rule_violations_are_refused_with_their_code() {
    let f = fixture().await;
    let mut body = leave_body(f.cl, "2026-12-01", "2026-12-01");
    body["reason"] = json!(" ");
    let (status, out) = post_leave(&f.app, &f.client, &body, &key()).await;
    assert_eq!(status, 422);
    assert_eq!(out["error"]["code"], "REASON_REQUIRED");

    let (status, out) = post_leave(
        &f.app,
        &f.client,
        &leave_body(f.cl, "2026-12-01", "2026-12-31"),
        &key(),
    )
    .await;
    assert_eq!(status, 422);
    assert_eq!(out["error"]["code"], "INSUFFICIENT_BALANCE");

    // Overlap with the employee's own pending request.
    let (status, _) = post_leave(
        &f.app,
        &f.client,
        &leave_body(f.cl, "2026-12-01", "2026-12-01"),
        &key(),
    )
    .await;
    assert_eq!(status, 201);
    let (status, out) = post_leave(
        &f.app,
        &f.client,
        &leave_body(f.cl, "2026-11-30", "2026-12-01"),
        &key(),
    )
    .await;
    assert_eq!(status, 422);
    assert_eq!(out["error"]["code"], "OVERLAP");

    // An external notify address must be a valid email.
    let mut body = leave_body(f.cl, "2026-12-07", "2026-12-07");
    body["notify"] = json!([{ "email": "not-an-email" }]);
    let (status, out) = post_leave(&f.app, &f.client, &body, &key()).await;
    assert_eq!(status, 422, "{out}");
    assert_eq!(out["error"]["code"], "VALIDATION");
    assert_eq!(out["error"]["details"]["field"], "notify");
}

#[tokio::test]
async fn attachment_required_above_the_types_threshold() {
    let f = fixture().await;
    set_type_rule(&f.app, f.cl, "attachment_required_above_days", "2").await;
    let (status, out) = post_leave(&f.app, &f.client, &body_with_notify(&f), &key()).await;
    assert_eq!(status, 422);
    assert_eq!(out["error"]["code"], "ATTACHMENT_REQUIRED");
    // Two days or fewer need none.
    let (status, out) = post_leave(
        &f.app,
        &f.client,
        &leave_body(f.cl, "2026-11-30", "2026-12-01"),
        &key(),
    )
    .await;
    assert_eq!(status, 201, "{out}");
}

#[tokio::test]
async fn no_approver_available_when_nobody_else_can_decide() {
    let app = TestApp::spawn().await;
    let (org, admin, client) = org_with_admin(&app, "solo.test").await;
    let cl = create_leave_type(&app, &org, NewLeaveType::all("CL", dec!(12))).await;
    add_ledger_row(&app, &admin, cl, 2026, dec!(12), "allotment").await;
    let (status, out) = post_leave(
        &app,
        &client,
        &leave_body(cl, "2026-12-01", "2026-12-01"),
        &key(),
    )
    .await;
    assert_eq!(status, 422, "{out}");
    assert_eq!(out["error"]["code"], "NO_APPROVER_AVAILABLE");
}

#[tokio::test]
async fn other_employees_cannot_see_the_request() {
    let f = fixture().await;
    let (_, out) = post_leave(&f.app, &f.client, &body_with_notify(&f), &key()).await;
    let url = f
        .app
        .url(&format!("/api/v1/leaves/{}", out["id"].as_str().unwrap()));
    let (status, _) = get_json(&f.app.login_as(&f.ravi).await, url.clone()).await;
    assert_eq!(status, 404);
    // Approvers of another group cannot either; the group's approver and the admin can.
    let (status, _) = get_json(&f.app.login_as(&f.vik).await, url.clone()).await;
    assert_eq!(status, 404);
    let (status, d) = get_json(&f.app.login_as(&f.priya).await, url.clone()).await;
    assert_eq!(status, 200);
    assert_eq!(d["reason"], "Family function");
    let (status, _) = get_json(&f.app.login_as(&f.admin).await, url).await;
    assert_eq!(status, 200);
    let _ = &f.org;
}
