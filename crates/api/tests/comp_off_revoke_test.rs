//! T148 — an admin revokes an unused comp-off credit (FR-061).
mod common;

use common::factories::*;
use common::*;
use rust_decimal_macros::dec;
use serde_json::{json, Value};
use uuid::Uuid;

struct World {
    app: TestApp,
    admin: reqwest::Client,
    priya: reqwest::Client,
    asha_user: User,
    asha: reqwest::Client,
    co: Uuid,
}

async fn world() -> World {
    let app = TestApp::spawn().await;
    let (org, _, admin) = org_with_admin(&app, "acme.test").await;
    let co = create_leave_type(&app, &org, NewLeaveType::comp_off()).await;
    let priya_u = create_user(
        &app,
        &org,
        NewUser::employee("priya@acme.test").role("approver"),
    )
    .await;
    let eng = create_group(&app, &org, "Eng", &[&priya_u]).await;
    let asha_user = create_user(&app, &org, NewUser::employee("asha@acme.test").group(eng)).await;
    let w = World {
        admin,
        priya: app.login_as(&priya_u).await,
        asha: app.login_as(&asha_user).await,
        asha_user,
        co,
        app,
    };
    earn_comp_off(
        &w.app,
        &w.asha,
        &w.priya,
        &[("2026-11-21", 1.0), ("2026-11-22", 1.0)],
    )
    .await;
    w
}

/// Asha's credits as an admin sees them.
async fn credits_of(w: &World) -> Vec<Value> {
    let (status, out) = get_json(
        &w.admin,
        w.app
            .url(&format!("/api/v1/comp-offs?user={}", w.asha_user.id)),
    )
    .await;
    assert_eq!(status, 200, "{out}");
    out["credits"].as_array().unwrap().clone()
}

async fn revoke(w: &World, client: &reqwest::Client, credit: &Value, body: Value) -> (u16, Value) {
    post_json(
        client,
        w.app.url(&format!(
            "/api/v1/admin/comp-off-credits/{}/revoke",
            credit["id"].as_str().unwrap()
        )),
        body,
    )
    .await
}

#[tokio::test]
async fn an_unused_credit_is_revoked_with_a_reason() {
    let w = world().await;
    let credit = credits_of(&w).await[0].clone();
    let (status, out) = revoke(&w, &w.admin, &credit, json!({})).await;
    assert_eq!(status, 422, "{out}");
    assert_eq!(out["error"]["code"], "NOTE_REQUIRED");
    let (status, out) = revoke(&w, &w.priya, &credit, json!({ "reason": "x" })).await;
    assert_eq!(status, 403, "{out}");

    let (status, out) = revoke(
        &w,
        &w.admin,
        &credit,
        json!({ "reason": "Worked from home" }),
    )
    .await;
    assert_eq!(status, 200, "{out}");
    assert_eq!(out["status"], "revoked");
    let after = credits_of(&w).await;
    assert_eq!(after[0]["status"], "revoked");
    assert_eq!(after[0]["revoke_reason"], "Worked from home");
    assert_eq!(comp_offs(&w.app, &w.asha).await["available"], 1.0);
    let revoked: Vec<_> = ledger_rows(&w.app, w.asha_user.id, w.co)
        .await
        .into_iter()
        .filter(|(r, ..)| r == "comp_off_revoked")
        .map(|(_, d, ..)| d)
        .collect();
    assert_eq!(revoked, [dec!(-1)]);
    let audit: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM audit_log WHERE action = 'comp_off.revoke' AND entity_id = $1::uuid",
    )
    .bind(credit["id"].as_str().unwrap())
    .fetch_one(&w.app.db.owner)
    .await
    .unwrap();
    assert_eq!(audit, 1);

    let (status, out) = revoke(&w, &w.admin, &credit, json!({ "reason": "Again" })).await;
    assert_eq!(status, 409, "{out}");
}

#[tokio::test]
async fn used_or_reserved_credit_cannot_be_revoked() {
    let w = world().await;
    // A pending 1-day comp-off leave reserves the first credit …
    let leave = submit(
        &w.app,
        &w.asha,
        &leave_body(w.co, "2026-11-30", "2026-11-30"),
    )
    .await;
    let credit = credits_of(&w).await[0].clone();
    let (status, out) = revoke(&w, &w.admin, &credit, json!({ "reason": "Wrong" })).await;
    assert_eq!(status, 422, "{out}");
    assert_eq!(out["error"]["code"], "CREDIT_IN_USE");
    // … and once approved it is used.
    decide(&w.app, &w.priya, &leave, approve()).await;
    let (status, out) = revoke(&w, &w.admin, &credit, json!({ "reason": "Wrong" })).await;
    assert_eq!(status, 422, "{out}");
    assert_eq!(out["error"]["code"], "CREDIT_IN_USE");
}

#[tokio::test]
async fn only_admins_read_someone_elses_credits() {
    let w = world().await;
    let (status, _) = get_json(
        &w.priya,
        w.app
            .url(&format!("/api/v1/comp-offs?user={}", w.asha_user.id)),
    )
    .await;
    assert_eq!(status, 403);
}
