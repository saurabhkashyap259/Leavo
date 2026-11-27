//! T114 — Approve/Reject links in approver emails (FR-024, research R5): pre-fill the decision,
//! valid 72 h, single use, and deciding still needs a signed-in session.
mod common;

use chrono::Duration;
use common::factories::*;
use common::*;
use rust_decimal_macros::dec;
use serde_json::json;

struct World {
    app: TestApp,
    admin: reqwest::Client,
    priya: reqwest::Client,
    id: String,
}

async fn world() -> World {
    let app = TestApp::spawn().await;
    let (org, _admin_user, admin) = org_with_admin(&app, "acme.test").await;
    let cl = create_leave_type(&app, &org, NewLeaveType::all("CL", dec!(12))).await;
    let priya_u = create_user(
        &app,
        &org,
        NewUser::employee("priya@acme.test").role("approver"),
    )
    .await;
    let eng = create_group(&app, &org, "Eng", &[&priya_u]).await;
    let asha = create_user(&app, &org, NewUser::employee("asha@acme.test").group(eng)).await;
    add_ledger_row(&app, &asha, cl, 2026, dec!(12), "allotment").await;
    let id = submit(
        &app,
        &app.login_as(&asha).await,
        &leave_body(cl, "2026-12-01", "2026-12-01"),
    )
    .await;
    let priya = app.login_as(&priya_u).await;
    World {
        app,
        admin,
        priya,
        id,
    }
}

/// The approve and reject tokens from the action email sent to `to`.
async fn links(w: &World, to: &str) -> (String, String) {
    w.app.run_jobs().await;
    let mail = w
        .app
        .email
        .sent()
        .into_iter()
        .find(|m| m.to == to && m.subject.contains("requested"))
        .expect("action email");
    let tokens: Vec<String> = mail
        .text
        .split_whitespace()
        .filter_map(|word| word.split_once("http://leavo.test/api/v1/approvals/action/"))
        .map(|(_, t)| t.to_string())
        .collect();
    assert_eq!(tokens.len(), 2, "{}", mail.text);
    // The HTML carries the same two links as buttons.
    for t in &tokens {
        assert!(mail.html.contains(t.as_str()));
    }
    (tokens[0].clone(), tokens[1].clone())
}

async fn follow(w: &World, token: &str) -> (u16, String) {
    let res = w
        .app
        .anon()
        .get(w.app.url(&format!("/api/v1/approvals/action/{token}")))
        .send()
        .await
        .unwrap();
    let location = res
        .headers()
        .get("location")
        .map(|v| v.to_str().unwrap().to_string())
        .unwrap_or_default();
    (res.status().as_u16(), location)
}

#[tokio::test]
async fn link_prefills_the_decision_and_works_once() {
    let w = world().await;
    let (approve_t, reject_t) = links(&w, "priya@acme.test").await;
    assert_ne!(approve_t, reject_t);

    // Opening the link (no session) only redirects to the inbox with the decision pre-filled.
    let (status, location) = follow(&w, &approve_t).await;
    assert_eq!(status, 303);
    assert_eq!(
        location,
        format!("/approvals?request={}&decision=approve&t={approve_t}", w.id)
    );
    let (_, location) = follow(&w, &reject_t).await;
    assert!(location.contains("decision=reject"), "{location}");
    // Link scanners may open it many times: still pending, link still usable.
    let (_, location) = follow(&w, &approve_t).await;
    assert!(location.contains("decision=approve"));

    // Deciding needs a session.
    let body = json!({ "decision": "approve", "version": 1, "action_token": approve_t });
    let (status, _) = decide(&w.app, &w.app.anon(), &w.id, body.clone()).await;
    assert_eq!(status, 401);

    // Another approver's token is not accepted.
    let (status, out) = decide(&w.app, &w.admin, &w.id, body.clone()).await;
    assert_eq!(
        (status, out["error"]["code"].as_str()),
        (403, Some("FORBIDDEN"))
    );
    // Nor a token for the other decision.
    let mismatched =
        json!({ "decision": "reject", "version": 1, "comment": "x", "action_token": approve_t });
    let (status, _) = decide(&w.app, &w.priya, &w.id, mismatched).await;
    assert_eq!(status, 403);

    let (status, out) = decide(&w.app, &w.priya, &w.id, body).await;
    assert_eq!(status, 200, "{out}");

    // Used: both links now land on a notice instead of a decision.
    assert_eq!(follow(&w, &approve_t).await.1, "/approvals?link=used");
    assert_eq!(follow(&w, &reject_t).await.1, "/approvals?link=used");
}

#[tokio::test]
async fn links_expire_after_72_hours() {
    let w = world().await;
    let (approve_t, _) = links(&w, "priya@acme.test").await;
    w.app
        .clock
        .advance(Duration::hours(72) - Duration::seconds(1));
    assert!(follow(&w, &approve_t).await.1.contains("decision=approve"));
    w.app.clock.advance(Duration::seconds(1));
    assert_eq!(follow(&w, &approve_t).await.1, "/approvals?link=expired");
    // An expired token cannot be used to decide either; the inbox still works without it.
    let (status, _) = decide(
        &w.app,
        &w.priya,
        &w.id,
        json!({ "decision": "approve", "version": 1, "action_token": approve_t }),
    )
    .await;
    assert_eq!(status, 403);
    let (status, _) = decide(&w.app, &w.priya, &w.id, approve()).await;
    assert_eq!(status, 200);
}

#[tokio::test]
async fn tampered_links_are_refused() {
    let w = world().await;
    let (approve_t, _) = links(&w, "priya@acme.test").await;
    let mut bad = approve_t.clone();
    let last = bad.pop().unwrap();
    bad.push(if last == 'A' { 'B' } else { 'A' });
    assert_eq!(follow(&w, &bad).await.1, "/approvals?link=invalid");
    assert_eq!(follow(&w, "garbage").await.1, "/approvals?link=invalid");
}

#[tokio::test]
async fn every_eligible_approver_gets_their_own_links() {
    let w = world().await;
    let (a1, _) = links(&w, "priya@acme.test").await;
    let (a2, _) = links(&w, "admin@acme.test").await;
    assert_ne!(a1, a2);
}
