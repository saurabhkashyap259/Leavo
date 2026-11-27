//! T179 — the audit log (FR-044): requests, decisions, balance changes, policy changes, role
//! changes and sign-out-all are recorded with who, when and before/after; filters and paging.
//! (The recovery-login entry is tested with that command in US12, T204.)
mod common;

use common::factories::*;
use common::*;
use rust_decimal_macros::dec;
use serde_json::{json, Value};

#[tokio::test]
async fn every_change_is_in_the_log_and_can_be_filtered() {
    let app = TestApp::spawn().await;
    let (org, admin_user, admin) = org_with_admin(&app, "acme.test").await;
    set_full_name(&app, &admin_user, "Ada Admin").await;
    let cl = create_leave_type(&app, &org, NewLeaveType::all("CL", dec!(12))).await;
    let asha = create_user(&app, &org, NewUser::employee("asha@acme.test")).await;
    add_ledger_row(&app, &asha, cl, 2026, dec!(12), "allotment").await;
    let asha_c = app.login_as(&asha).await;

    let id = submit(&app, &asha_c, &leave_body(cl, "2026-11-30", "2026-11-30")).await;
    decide(&app, &admin, &id, approve()).await;
    post_json(
        &admin,
        app.url(&format!("/api/v1/admin/users/{}/adjustments", asha.id)),
        json!({ "leave_type_id": cl, "leave_year": 2026, "delta": 1, "note": "Bonus day" }),
    )
    .await;
    let (status, _) = patch_json(
        &admin,
        app.url(&format!("/api/v1/admin/leave-types/{cl}")),
        json!({ "name": "Casual" }),
    )
    .await;
    assert_eq!(status, 200);
    patch_json(
        &admin,
        app.url(&format!("/api/v1/admin/users/{}", asha.id)),
        json!({ "role": "approver" }),
    )
    .await;

    let log = |q: &str| {
        let (client, url) = (
            admin.clone(),
            app.url(&format!("/api/v1/admin/audit-log?{q}")),
        );
        async move {
            let (status, out) = get_json(&client, url).await;
            assert_eq!(status, 200, "{out}");
            out
        }
    };
    let actions = |v: &Value| -> Vec<String> {
        v["items"]
            .as_array()
            .unwrap()
            .iter()
            .map(|i| i["action"].as_str().unwrap().into())
            .collect()
    };
    let all = log("").await;
    // Newest first.
    assert_eq!(
        actions(&all)[..5],
        [
            "user.update",
            "policy.update",
            "balance.adjust",
            "leave.approve",
            "leave.request"
        ]
    );
    let role = &all["items"][0];
    assert_eq!(role["actor"]["full_name"], "Ada Admin");
    assert_eq!(role["entity_kind"], "user");
    assert_eq!(role["entity_id"], asha.id.to_string());
    assert_eq!(role["before"]["role"], "employee");
    assert_eq!(role["after"]["role"], "approver");
    assert!(role["created_at"].is_string());
    let adjust = &all["items"][2];
    assert_eq!(adjust["before"]["balance"], 11.0);
    assert_eq!(adjust["after"]["balance"], 12.0);
    assert_eq!(adjust["after"]["note"], "Bonus day");

    // Filters: actor, entity, action, dates.
    assert_eq!(
        actions(&log(&format!("actor={}", asha.id)).await),
        ["leave.request"]
    );
    assert_eq!(
        actions(&log(&format!("entity={id}")).await),
        ["leave.approve", "leave.request"]
    );
    assert_eq!(
        actions(&log("action=balance.adjust").await),
        ["balance.adjust"]
    );
    assert_eq!(actions(&log("from=2030-01-01").await), Vec::<String>::new());
    // Audit times are the database's real time, not the test clock.
    let all_time = log("from=2000-01-01&to=2100-12-31").await;
    assert_eq!(actions(&all_time), actions(&all));

    // Paging.
    let first = log("limit=2").await;
    assert_eq!(actions(&first), actions(&all)[..2]);
    let cursor = first["next_cursor"].as_str().unwrap().to_string();
    let second = log(&format!("limit=2&cursor={cursor}")).await;
    assert_eq!(actions(&second), actions(&all)[2..4]);

    // Sign-out-all is recorded even though it ends the caller's session.
    let (status, _) = post_json(&admin, app.url("/api/v1/admin/org/sign-out-all"), json!({})).await;
    assert_eq!(status, 204);
    let admin = app.login_as(&admin_user).await;
    let (_, out) = get_json(
        &admin,
        app.url("/api/v1/admin/audit-log?action=org.sign_out_all"),
    )
    .await;
    assert_eq!(out["items"][0]["actor"]["full_name"], "Ada Admin");

    let (status, _) = get_json(&asha_c, app.url("/api/v1/admin/audit-log")).await;
    assert_eq!(status, 401, "signed out by sign-out-all");
    for q in ["from=yesterday", "actor=nobody", "cursor=nonsense"] {
        let (status, _) = get_json(&admin, app.url(&format!("/api/v1/admin/audit-log?{q}"))).await;
        assert_eq!(status, 422, "{q}");
    }
}
