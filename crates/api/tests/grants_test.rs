//! T073 — restricted leave types granted per employee (FR-012, clarification Q5).
mod common;

use common::factories::*;
use common::*;
use rust_decimal_macros::dec;
use serde_json::json;

fn codes(balances: &serde_json::Value) -> Vec<String> {
    balances
        .as_array()
        .unwrap()
        .iter()
        .map(|b| b["leave_type"]["code"].as_str().unwrap().to_string())
        .collect()
}

#[tokio::test]
async fn grant_allots_the_full_year_and_shows_the_type_only_to_the_grantee() {
    let app = TestApp::spawn().await;
    let (org, admin_user, admin) = org_with_admin(&app, "acme.test").await;
    create_leave_type(&app, &org, NewLeaveType::all("CL", dec!(12))).await;
    let ml = create_leave_type(&app, &org, NewLeaveType::restricted("ML", dec!(130))).await;
    let asha = create_user(&app, &org, NewUser::employee("asha@acme.test")).await;
    let ravi = create_user(&app, &org, NewUser::employee("ravi@acme.test")).await;

    let url = app.url(&format!("/api/v1/admin/leave-types/{ml}/grants"));
    let (status, grant) = post_json(&admin, url.clone(), json!({ "user_id": asha.id })).await;
    assert_eq!(status, 201, "{grant}");
    assert_eq!(grant["user_id"], asha.id.to_string());
    assert_eq!(grant["granted_by"], admin_user.id.to_string());

    // Full yearly quota for the current leave year (2026), not pro-rated in late November.
    assert_eq!(
        ledger_rows(&app, asha.id, ml).await,
        [(
            "allotment".to_string(),
            dec!(130),
            2026,
            Some("grant".to_string())
        )]
    );

    let (_, asha_bal) = get_json(&app.login_as(&asha).await, app.url("/api/v1/me/balances")).await;
    assert_eq!(codes(&asha_bal), ["CL", "ML"]);
    let ml_card = asha_bal
        .as_array()
        .unwrap()
        .iter()
        .find(|b| b["leave_type"]["code"] == "ML")
        .unwrap();
    assert_eq!(ml_card["allotted"], 130.0);
    assert_eq!(ml_card["available"], 130.0);

    let (_, ravi_bal) = get_json(&app.login_as(&ravi).await, app.url("/api/v1/me/balances")).await;
    assert_eq!(codes(&ravi_bal), ["CL"]);

    let (status, list) = get_json(&admin, url).await;
    assert_eq!(status, 200);
    assert_eq!(list.as_array().unwrap().len(), 1);
    assert_eq!(list[0]["email"], "asha@acme.test");

    let (_, types) = get_json(&admin, app.url("/api/v1/admin/leave-types")).await;
    let ml_row = types
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["code"] == "ML")
        .unwrap();
    assert_eq!(ml_row["granted_count"], 1);
}

#[tokio::test]
async fn grant_by_email_is_accepted() {
    let app = TestApp::spawn().await;
    let (org, _a, admin) = org_with_admin(&app, "acme.test").await;
    let ml = create_leave_type(&app, &org, NewLeaveType::restricted("ML", dec!(130))).await;
    let asha = create_user(&app, &org, NewUser::employee("asha@acme.test")).await;
    let (status, grant) = post_json(
        &admin,
        app.url(&format!("/api/v1/admin/leave-types/{ml}/grants")),
        json!({ "email": "ASHA@acme.test" }),
    )
    .await;
    assert_eq!(status, 201, "{grant}");
    assert_eq!(grant["user_id"], asha.id.to_string());
}

#[tokio::test]
async fn grants_are_only_for_restricted_types_and_same_org_users() {
    let app = TestApp::spawn().await;
    let (org, _a, admin) = org_with_admin(&app, "acme.test").await;
    let cl = create_leave_type(&app, &org, NewLeaveType::all("CL", dec!(12))).await;
    let ml = create_leave_type(&app, &org, NewLeaveType::restricted("ML", dec!(130))).await;
    let asha = create_user(&app, &org, NewUser::employee("asha@acme.test")).await;
    let other_org = create_org(&app, "Beta", "beta.test").await;
    let outsider = create_user(&app, &other_org, NewUser::employee("x@beta.test")).await;

    let (status, body) = post_json(
        &admin,
        app.url(&format!("/api/v1/admin/leave-types/{cl}/grants")),
        json!({ "user_id": asha.id }),
    )
    .await;
    assert_eq!(
        (status, body["error"]["code"].as_str()),
        (422, Some("VALIDATION"))
    );

    let (status, _) = post_json(
        &admin,
        app.url(&format!("/api/v1/admin/leave-types/{ml}/grants")),
        json!({ "user_id": outsider.id }),
    )
    .await;
    assert_eq!(status, 404);

    // Granting twice is idempotent: one active grant, one allotment.
    let url = app.url(&format!("/api/v1/admin/leave-types/{ml}/grants"));
    post_json(&admin, url.clone(), json!({ "user_id": asha.id })).await;
    let (status, _) = post_json(&admin, url, json!({ "user_id": asha.id })).await;
    assert_eq!(status, 200);
    assert_eq!(ledger_rows(&app, asha.id, ml).await.len(), 1);
}

#[tokio::test]
async fn revoke_lapses_the_remaining_balance_and_keeps_history() {
    let app = TestApp::spawn().await;
    let (org, _a, admin) = org_with_admin(&app, "acme.test").await;
    let ml = create_leave_type(&app, &org, NewLeaveType::restricted("ML", dec!(130))).await;
    let asha = create_user(&app, &org, NewUser::employee("asha@acme.test")).await;
    let grants = app.url(&format!("/api/v1/admin/leave-types/{ml}/grants"));
    post_json(&admin, grants.clone(), json!({ "user_id": asha.id })).await;
    // Some of it was already used.
    add_ledger_row(&app, &asha, ml, 2026, dec!(-30), "leave_taken").await;

    let (status, _) = delete(
        &admin,
        app.url(&format!(
            "/api/v1/admin/leave-types/{ml}/grants/{}",
            asha.id
        )),
    )
    .await;
    assert_eq!(status, 204);

    let rows = ledger_rows(&app, asha.id, ml).await;
    assert_eq!(rows.len(), 3, "allotment and usage are kept");
    assert_eq!(
        rows[2],
        (
            "lapse".to_string(),
            dec!(-100),
            2026,
            Some("grant".to_string())
        )
    );

    let (_, bal) = get_json(&app.login_as(&asha).await, app.url("/api/v1/me/balances")).await;
    assert!(codes(&bal).is_empty(), "type hidden after revoke");

    // Active list is empty; history keeps the revoked grant.
    let (_, active) = get_json(&admin, grants.clone()).await;
    assert!(active.as_array().unwrap().is_empty());
    let (_, all) = get_json(&admin, format!("{grants}?include_revoked=true")).await;
    assert_eq!(all.as_array().unwrap().len(), 1);
    assert!(all[0]["revoked_at"].is_string());

    // Re-granting in the same leave year does not allot again (once per user/type/year).
    let (status, _) = post_json(&admin, grants, json!({ "user_id": asha.id })).await;
    assert_eq!(status, 201);
    assert_eq!(ledger_rows(&app, asha.id, ml).await.len(), 3);

    // Revoking something that is not granted → 404.
    let ravi = create_user(&app, &org, NewUser::employee("ravi@acme.test")).await;
    let (status, _) = delete(
        &admin,
        app.url(&format!(
            "/api/v1/admin/leave-types/{ml}/grants/{}",
            ravi.id
        )),
    )
    .await;
    assert_eq!(status, 404);
}

#[tokio::test]
async fn revoke_keeps_enough_for_pending_requests() {
    let app = TestApp::spawn().await;
    let (org, _a, admin) = org_with_admin(&app, "acme.test").await;
    let ml = create_leave_type(&app, &org, NewLeaveType::restricted("ML", dec!(130))).await;
    let asha = create_user(&app, &org, NewUser::employee("asha@acme.test")).await;
    let grants = app.url(&format!("/api/v1/admin/leave-types/{ml}/grants"));
    post_json(&admin, grants, json!({ "user_id": asha.id })).await;

    // Asha has 3 days pending (Mon 30 Nov – Wed 2 Dec) when the grant is revoked.
    let asha_client = app.login_as(&asha).await;
    let (status, out) = post_leave(
        &app,
        &asha_client,
        &leave_body(ml, "2026-11-30", "2026-12-02"),
        &key(),
    )
    .await;
    assert_eq!(status, 201, "{out}");

    let (status, _) = delete(
        &admin,
        app.url(&format!(
            "/api/v1/admin/leave-types/{ml}/grants/{}",
            asha.id
        )),
    )
    .await;
    assert_eq!(status, 204);
    // Lapse = −(balance − pending): the pending days stay covered (FR-012, Edge Cases).
    let rows = ledger_rows(&app, asha.id, ml).await;
    assert_eq!(
        rows.last().unwrap(),
        &(
            "lapse".to_string(),
            dec!(-127),
            2026,
            Some("grant".to_string())
        )
    );
    let (status,): (String,) = sqlx::query_as("SELECT status::text FROM leave_requests")
        .fetch_one(&app.db.owner)
        .await
        .unwrap();
    assert_eq!(status, "pending");
}
