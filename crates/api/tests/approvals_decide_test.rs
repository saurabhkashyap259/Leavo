//! T112 — deciding a request (FR-018, FR-023, FR-046, FR-055, Edge Cases).
mod common;

use common::factories::*;
use common::*;
use rust_decimal_macros::dec;
use serde_json::json;
use uuid::Uuid;

struct World {
    app: TestApp,
    org: Org,
    admin_user: User,
    admin: reqwest::Client,
    priya_user: User,
    priya: reqwest::Client,
    vik: reqwest::Client,
    asha: User,
    asha_client: reqwest::Client,
    ravi: User,
    cl: Uuid,
}

async fn world() -> World {
    let app = TestApp::spawn().await;
    let (org, admin_user, admin) = org_with_admin(&app, "acme.test").await;
    let cl = create_leave_type(&app, &org, NewLeaveType::all("CL", dec!(12))).await;
    sqlx::query("UPDATE leave_types SET name = 'Casual Leave' WHERE id = $1")
        .bind(cl)
        .execute(&app.db.owner)
        .await
        .unwrap();
    let priya_user = create_user(
        &app,
        &org,
        NewUser::employee("priya@acme.test").role("approver"),
    )
    .await;
    set_full_name(&app, &priya_user, "Priya Shah").await;
    let vik_u = create_user(
        &app,
        &org,
        NewUser::employee("vik@acme.test").role("approver"),
    )
    .await;
    let eng = create_group(&app, &org, "Eng", &[&priya_user]).await;
    create_group(&app, &org, "Sales", &[&vik_u]).await;
    let asha = create_user(&app, &org, NewUser::employee("asha@acme.test").group(eng)).await;
    set_full_name(&app, &asha, "Asha Rao").await;
    let ravi = create_user(&app, &org, NewUser::employee("ravi@acme.test")).await;
    add_ledger_row(&app, &asha, cl, 2026, dec!(12), "allotment").await;
    let priya = app.login_as(&priya_user).await;
    let vik = app.login_as(&vik_u).await;
    let asha_client = app.login_as(&asha).await;
    World {
        app,
        org,
        admin_user,
        admin,
        priya_user,
        priya,
        vik,
        asha,
        asha_client,
        ravi,
        cl,
    }
}

fn request_with_notify(w: &World, from: &str, to: &str) -> serde_json::Value {
    let mut body = leave_body(w.cl, from, to);
    body["reason"] = json!("Family function");
    body["notify"] = json!([{ "user_id": w.ravi.id }]);
    body
}

#[tokio::test]
async fn approve_deducts_days_per_leave_year_and_emails_with_a_calendar_invite() {
    let w = world().await;
    let id = submit(
        &w.app,
        &w.asha_client,
        &request_with_notify(&w, "2026-11-30", "2026-12-02"),
    )
    .await;
    w.app.run_jobs().await;
    w.app.email.take();

    let (status, out) = decide(&w.app, &w.priya, &id, approve()).await;
    assert_eq!(status, 200, "{out}");
    assert_eq!(out["status"], "approved");
    assert_eq!(out["decided_by"]["full_name"], "Priya Shah");
    assert_eq!(out["decided_at"], "2026-11-24T09:00:00Z");

    assert_eq!(
        ledger_rows(&w.app, w.asha.id, w.cl).await[1..],
        [(
            "leave_taken".to_string(),
            dec!(-3),
            2026,
            Some("leave_request".to_string())
        )]
    );
    let card = balance_card(&w.app, &w.asha_client, "CL", None).await;
    assert_eq!(
        (
            card["taken"].clone(),
            card["pending"].clone(),
            card["available"].clone()
        ),
        (json!(3.0), json!(0.0), json!(9.0))
    );
    let (_, detail) = get_json(&w.asha_client, w.app.url(&format!("/api/v1/leaves/{id}"))).await;
    assert_eq!(detail["status"], "approved");
    assert_eq!(detail["decided_by"]["full_name"], "Priya Shah");
    assert_eq!(detail["version"], 2);

    w.app.run_jobs().await;
    let sent = w.app.email.take();
    let mine = sent
        .iter()
        .find(|m| m.to == "asha@acme.test")
        .expect("employee email");
    assert!(mine.subject.contains("approved"), "{}", mine.subject);
    assert!(mine.text.contains("Priya Shah"));
    assert_eq!(mine.attachments.len(), 1);
    let ics = String::from_utf8(mine.attachments[0].bytes.clone()).unwrap();
    assert_eq!(
        mine.attachments[0].content_type,
        "text/calendar; charset=utf-8; method=PUBLISH"
    );
    assert!(ics.contains("BEGIN:VCALENDAR"), "{ics}");
    assert!(ics.contains("DTSTART;VALUE=DATE:20261130"), "{ics}");
    assert!(ics.contains("DTEND;VALUE=DATE:20261203"), "{ics}");
    assert!(ics.contains("Casual Leave"), "{ics}");

    // Notified members: minimal content, neutral invite (FR-059).
    let fyi = sent
        .iter()
        .find(|m| m.to == "ravi@acme.test")
        .expect("FYI email");
    for part in [&fyi.subject, &fyi.text, &fyi.html] {
        assert!(!part.contains("Casual"), "{part}");
        assert!(!part.contains("Family function"), "{part}");
        assert!(!part.contains("http"), "{part}");
    }
    assert!(fyi.text.to_lowercase().contains("approved"));
    let fyi_ics = String::from_utf8(fyi.attachments[0].bytes.clone()).unwrap();
    assert!(fyi_ics.contains("Asha Rao"), "{fyi_ics}");
    assert!(!fyi_ics.contains("Casual"), "{fyi_ics}");
    assert_eq!(
        sent.len(),
        2,
        "{:?}",
        sent.iter().map(|m| &m.to).collect::<Vec<_>>()
    );
}

#[tokio::test]
async fn a_request_across_leave_years_is_charged_to_each_year() {
    let w = world().await;
    let id = submit(
        &w.app,
        &w.asha_client,
        &leave_body(w.cl, "2026-12-29", "2027-01-02"),
    )
    .await;
    let (status, _) = decide(&w.app, &w.admin, &id, approve()).await;
    assert_eq!(status, 200);
    let rows = ledger_rows(&w.app, w.asha.id, w.cl).await;
    let taken: Vec<_> = rows
        .iter()
        .filter(|r| r.0 == "leave_taken")
        .map(|r| (r.1, r.2))
        .collect();
    assert_eq!(taken, [(dec!(-3), 2026), (dec!(-1), 2027)]);
}

#[tokio::test]
async fn reject_needs_a_comment_and_releases_the_days() {
    let w = world().await;
    let id = submit(
        &w.app,
        &w.asha_client,
        &request_with_notify(&w, "2026-11-30", "2026-12-02"),
    )
    .await;
    let (status, out) = decide(
        &w.app,
        &w.priya,
        &id,
        json!({ "decision": "reject", "version": 1 }),
    )
    .await;
    assert_eq!(status, 422);
    assert_eq!(out["error"]["code"], "COMMENT_REQUIRED");
    let (status, _) = decide(
        &w.app,
        &w.priya,
        &id,
        json!({ "decision": "reject", "version": 1, "comment": "  " }),
    )
    .await;
    assert_eq!(status, 422);

    w.app.run_jobs().await;
    w.app.email.take();
    let (status, out) = decide(
        &w.app,
        &w.priya,
        &id,
        json!({ "decision": "reject", "version": 1, "comment": "Release week" }),
    )
    .await;
    assert_eq!(status, 200, "{out}");
    assert_eq!(out["status"], "rejected");
    let card = balance_card(&w.app, &w.asha_client, "CL", None).await;
    assert_eq!(
        (card["pending"].clone(), card["available"].clone()),
        (json!(0.0), json!(12.0))
    );
    assert_eq!(
        ledger_rows(&w.app, w.asha.id, w.cl).await.len(),
        1,
        "no ledger rows"
    );

    w.app.run_jobs().await;
    let sent = w.app.email.take();
    let mine = sent.iter().find(|m| m.to == "asha@acme.test").unwrap();
    assert!(mine.subject.contains("rejected"), "{}", mine.subject);
    assert!(mine.text.contains("Release week"));
    assert!(mine.attachments.is_empty());
    let fyi = sent.iter().find(|m| m.to == "ravi@acme.test").unwrap();
    assert!(
        !fyi.text.contains("Release week"),
        "the comment is not for notified members"
    );

    // The same dates can be requested again.
    submit(
        &w.app,
        &w.asha_client,
        &leave_body(w.cl, "2026-11-30", "2026-12-02"),
    )
    .await;
}

#[tokio::test]
async fn who_may_decide() {
    let w = world().await;
    let id = submit(
        &w.app,
        &w.asha_client,
        &leave_body(w.cl, "2026-12-01", "2026-12-01"),
    )
    .await;

    let (status, out) = decide(&w.app, &w.vik, &id, approve()).await;
    assert_eq!(
        (status, out["error"]["code"].as_str()),
        (403, Some("NOT_ELIGIBLE_APPROVER"))
    );
    let (status, out) = decide(&w.app, &w.app.login_as(&w.ravi).await, &id, approve()).await;
    assert_eq!(
        (status, out["error"]["code"].as_str()),
        (403, Some("NOT_ELIGIBLE_APPROVER"))
    );
    let (status, _) = decide(&w.app, &w.app.anon(), &id, approve()).await;
    assert_eq!(status, 401);

    // The admin's own request goes to others; the admin cannot decide it.
    add_ledger_row(&w.app, &w.admin_user, w.cl, 2026, dec!(12), "allotment").await;
    let own = submit(
        &w.app,
        &w.admin,
        &leave_body(w.cl, "2026-12-03", "2026-12-03"),
    )
    .await;
    let (status, out) = decide(&w.app, &w.admin, &own, approve()).await;
    assert_eq!(
        (status, out["error"]["code"].as_str()),
        (403, Some("SELF_APPROVAL"))
    );
    // With no group, every approver may decide it.
    let (status, _) = decide(&w.app, &w.vik, &own, approve()).await;
    assert_eq!(status, 200);

    // Unknown request or kind → 404.
    let (status, _) = decide(&w.app, &w.admin, &Uuid::now_v7().to_string(), approve()).await;
    assert_eq!(status, 404);
    let res = w
        .admin
        .post(w.app.url(&format!("/api/v1/approvals/holiday/{id}/decide")))
        .header("Idempotency-Key", key())
        .json(&approve())
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 404);
}

#[tokio::test]
async fn a_stale_version_is_told_who_decided() {
    let w = world().await;
    let id = submit(
        &w.app,
        &w.asha_client,
        &leave_body(w.cl, "2026-12-01", "2026-12-01"),
    )
    .await;
    let (status, _) = decide(&w.app, &w.priya, &id, approve()).await;
    assert_eq!(status, 200);
    let (status, out) = decide(
        &w.app,
        &w.admin,
        &id,
        json!({ "decision": "reject", "version": 1, "comment": "No" }),
    )
    .await;
    assert_eq!(status, 409, "{out}");
    assert_eq!(out["error"]["code"], "ALREADY_DECIDED");
    let d = &out["error"]["details"];
    assert_eq!(d["status"], "approved");
    assert_eq!(d["decided_by"]["full_name"], "Priya Shah");
    assert_eq!(d["decided_at"], "2026-11-24T09:00:00Z");
    // Only one ledger deduction.
    assert_eq!(ledger_rows(&w.app, w.asha.id, w.cl).await.len(), 2);
}

#[tokio::test]
async fn the_idempotency_key_replays_the_decision() {
    let w = world().await;
    let id = submit(
        &w.app,
        &w.asha_client,
        &leave_body(w.cl, "2026-12-01", "2026-12-01"),
    )
    .await;
    let k = key();
    let first = decide_with_key(&w.app, &w.priya, &id, approve(), &k).await;
    let again = decide_with_key(&w.app, &w.priya, &id, approve(), &k).await;
    assert_eq!(first.0, 200);
    assert_eq!(first, again);
    let (status, _) = w
        .priya
        .post(w.app.url(&format!("/api/v1/approvals/leave/{id}/decide")))
        .json(&approve())
        .send()
        .await
        .map(|r| (r.status().as_u16(), ()))
        .unwrap();
    assert_eq!(status, 422, "Idempotency-Key is required");
}

#[tokio::test]
async fn a_revoked_grant_does_not_block_the_decision() {
    let w = world().await;
    let ml = create_leave_type(&w.app, &w.org, NewLeaveType::restricted("ML", dec!(130))).await;
    let grants = w.app.url(&format!("/api/v1/admin/leave-types/{ml}/grants"));
    post_json(&w.admin, grants, json!({ "user_id": w.asha.id })).await;
    let id = submit(
        &w.app,
        &w.asha_client,
        &leave_body(ml, "2026-12-01", "2026-12-02"),
    )
    .await;
    delete(
        &w.admin,
        w.app.url(&format!(
            "/api/v1/admin/leave-types/{ml}/grants/{}",
            w.asha.id
        )),
    )
    .await;
    let (status, out) = decide(&w.app, &w.priya, &id, approve()).await;
    assert_eq!(status, 200, "{out}");
    let rows = ledger_rows(&w.app, w.asha.id, ml).await;
    assert_eq!(rows.last().unwrap().0, "leave_taken");
    // allotment 130 − lapse 128 − taken 2 = 0.
    assert_eq!(
        rows.iter().map(|r| r.1).sum::<rust_decimal::Decimal>(),
        dec!(0)
    );
}

#[tokio::test]
async fn others_can_decide_when_an_approver_is_deactivated() {
    let w = world().await;
    let id = submit(
        &w.app,
        &w.asha_client,
        &leave_body(w.cl, "2026-12-01", "2026-12-01"),
    )
    .await;
    sqlx::query("UPDATE users SET status = 'deactivated' WHERE id = $1")
        .bind(w.priya_user.id)
        .execute(&w.app.db.owner)
        .await
        .unwrap();
    let (status, _) = decide(&w.app, &w.admin, &id, approve()).await;
    assert_eq!(status, 200);
}
