//! T085 — `/me` profile edits and `/me/balances` (FR-026, FR-064, FR-012).
mod common;

use common::factories::*;
use common::*;
use rust_decimal_macros::dec;
use serde_json::json;

#[tokio::test]
async fn balances_show_types_available_to_the_user_with_ledger_figures() {
    let app = TestApp::spawn().await;
    let org = create_org(&app, "Acme", "acme.test").await;
    let cl = create_leave_type(&app, &org, NewLeaveType::all("CL", dec!(12))).await;
    let _sl = create_leave_type(&app, &org, NewLeaveType::all("SL", dec!(6))).await;
    create_leave_type(&app, &org, NewLeaveType::all("OLD", dec!(3)).inactive()).await;
    create_leave_type(&app, &org, NewLeaveType::restricted("ML", dec!(130))).await;
    create_leave_type(&app, &org, NewLeaveType::comp_off()).await;
    let u = create_user(&app, &org, NewUser::employee("asha@acme.test")).await;
    add_ledger_row(&app, &u, cl, 2026, dec!(12), "allotment").await;
    add_ledger_row(&app, &u, cl, 2025, dec!(3), "carry_forward").await; // other year: ignored
    add_ledger_row(&app, &u, cl, 2026, dec!(2), "carry_forward").await;
    add_ledger_row(&app, &u, cl, 2026, dec!(-1.5), "leave_taken").await;

    let client = app.login_as(&u).await;
    let (status, body) = get_json(&client, app.url("/api/v1/me/balances")).await;
    assert_eq!(status, 200, "{body}");
    let rows = body.as_array().unwrap();
    let codes: Vec<&str> = rows
        .iter()
        .map(|r| r["leave_type"]["code"].as_str().unwrap())
        .collect();
    assert_eq!(codes, ["CL", "SL", "CO"]);
    let card = &rows[0];
    assert_eq!(card["leave_year"], 2026);
    assert_eq!(card["leave_type"]["color"], "#3B82F6");
    assert_eq!(card["allotted"], 12.0);
    assert_eq!(card["carried"], 2.0);
    assert_eq!(card["taken"], 1.5);
    assert_eq!(card["adjusted"], 0.0);
    assert_eq!(card["pending"], 0.0);
    assert_eq!(card["available"], 12.5);
    assert_eq!(rows[1]["available"], 0.0);
    assert_eq!(rows[2]["leave_type"]["is_comp_off"], true);

    let (_, last_year) = get_json(&client, app.url("/api/v1/me/balances?year=2025")).await;
    assert_eq!(last_year[0]["leave_year"], 2025);
    assert_eq!(last_year[0]["carried"], 3.0);
}

#[tokio::test]
async fn profile_fields_can_be_edited_except_joining_date() {
    let app = TestApp::spawn().await;
    let org = create_org(&app, "Acme", "acme.test").await;
    let u = create_user(&app, &org, NewUser::employee("asha@acme.test")).await;
    let client = app.login_as(&u).await;

    let (status, me) = patch_json(
        &client,
        app.url("/api/v1/me"),
        json!({ "full_name": " Asha Rao ", "department": "Engineering", "employee_code": "E-042" }),
    )
    .await;
    assert_eq!(status, 200, "{me}");
    assert_eq!(me["full_name"], "Asha Rao");
    assert_eq!(me["department"], "Engineering");
    assert_eq!(me["employee_code"], "E-042");

    let (status, body) =
        patch_json(&client, app.url("/api/v1/me"), json!({ "full_name": "  " })).await;
    assert_eq!(
        (status, body["error"]["details"]["field"].as_str()),
        (422, Some("full_name"))
    );

    // Unknown fields (date_of_joining) are rejected, not silently ignored.
    let (status, _) = patch_json(
        &client,
        app.url("/api/v1/me"),
        json!({ "date_of_joining": "2020-01-01" }),
    )
    .await;
    assert_eq!(status, 422);

    // Employee codes are unique within the org.
    let other = create_user(&app, &org, NewUser::employee("ravi@acme.test")).await;
    let (status, body) = patch_json(
        &app.login_as(&other).await,
        app.url("/api/v1/me"),
        json!({ "employee_code": "E-042" }),
    )
    .await;
    assert_eq!(
        (status, body["error"]["details"]["field"].as_str()),
        (422, Some("employee_code"))
    );
}

#[tokio::test]
async fn me_lists_the_last_notified_members_for_the_next_request() {
    let app = TestApp::spawn().await;
    let (org, _admin, _) = org_with_admin(&app, "acme.test").await;
    let cl = create_leave_type(&app, &org, NewLeaveType::all("CL", dec!(12))).await;
    let ravi = create_user(&app, &org, NewUser::employee("ravi@acme.test")).await;
    let asha = create_user(&app, &org, NewUser::employee("asha@acme.test")).await;
    add_ledger_row(&app, &asha, cl, 2026, dec!(12), "allotment").await;
    let client = app.login_as(&asha).await;

    let (_, me) = get_json(&client, app.url("/api/v1/me")).await;
    assert_eq!(me["last_notify"], json!([]));

    let mut body = leave_body(cl, "2026-12-01", "2026-12-01");
    body["notify"] = json!([{ "user_id": ravi.id }, { "email": "client@x.com" }]);
    let (status, out) = post_leave(&app, &client, &body, &key()).await;
    assert_eq!(status, 201, "{out}");

    let (_, me) = get_json(&client, app.url("/api/v1/me")).await;
    assert_eq!(
        me["last_notify"],
        json!([
            { "user_id": ravi.id, "email": "ravi@acme.test" },
            { "user_id": null, "email": "client@x.com" },
        ])
    );
}
