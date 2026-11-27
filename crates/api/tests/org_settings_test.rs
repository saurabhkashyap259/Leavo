//! T074 — organization settings (FR-010, FR-062).
mod common;

use common::factories::*;
use common::*;
use rust_decimal_macros::dec;
use serde_json::json;

#[tokio::test]
async fn admin_reads_and_updates_settings() {
    let app = TestApp::spawn().await;
    let (_org, _a, admin) = org_with_admin(&app, "acme.test").await;
    let url = app.url("/api/v1/admin/org");

    let (status, org) = get_json(&admin, url.clone()).await;
    assert_eq!(status, 200);
    assert_eq!(org["timezone"], "Asia/Kolkata");
    assert_eq!(org["weekend"], json!(["sat", "sun"]));
    assert_eq!(org["leave_year_start_month"], 1);
    assert_eq!(org["carry_forward_cap"], 0.0);
    assert_eq!(org["comp_off_expiry_days"], 90);

    let patch = json!({
        "name": "Acme Labs", "timezone": "Europe/London", "weekend": ["fri", "sat"],
        "carry_forward_cap": 5, "comp_off_expiry_days": 60, "leave_year_start_month": 4
    });
    let (status, updated) = patch_json(&admin, url.clone(), patch.clone()).await;
    assert_eq!(status, 200, "{updated}");
    for (k, v) in patch.as_object().unwrap() {
        let got = &updated[k];
        assert!(got == v || got.as_f64() == v.as_f64(), "{k}: {got} vs {v}");
    }
    let (_, me) = get_json(&admin, app.url("/api/v1/me")).await;
    assert_eq!(me["org"]["weekend"], json!(["fri", "sat"]));
}

#[tokio::test]
async fn settings_are_validated() {
    let app = TestApp::spawn().await;
    let (_org, _a, admin) = org_with_admin(&app, "acme.test").await;
    let url = app.url("/api/v1/admin/org");
    for (field, value) in [
        ("timezone", json!("Mars/Olympus")),
        ("weekend", json!(["sat", "funday"])),
        (
            "weekend",
            json!(["mon", "tue", "wed", "thu", "fri", "sat", "sun"]),
        ),
        ("leave_year_start_month", json!(0)),
        ("carry_forward_cap", json!(-1)),
        ("comp_off_expiry_days", json!(400)),
        ("name", json!("")),
    ] {
        let (status, body) = patch_json(&admin, url.clone(), json!({ field: value })).await;
        assert_eq!(status, 422, "{field}={value}");
        assert_eq!(body["error"]["details"]["field"], field);
    }
}

#[tokio::test]
async fn leave_year_start_is_locked_once_leave_data_exists() {
    let app = TestApp::spawn().await;
    let (org, admin_user, admin) = org_with_admin(&app, "acme.test").await;
    let cl = create_leave_type(&app, &org, NewLeaveType::all("CL", dec!(12))).await;
    let url = app.url("/api/v1/admin/org");

    // Initial allotments alone do not lock it.
    add_ledger_row(&app, &admin_user, cl, 2026, dec!(12), "allotment").await;
    add_ledger_row(&app, &admin_user, cl, 2026, dec!(6), "prorate").await;
    let (status, _) = patch_json(&admin, url.clone(), json!({ "leave_year_start_month": 4 })).await;
    assert_eq!(status, 200);

    add_ledger_row(&app, &admin_user, cl, 2026, dec!(1), "manual_adjustment").await;
    let (status, body) =
        patch_json(&admin, url.clone(), json!({ "leave_year_start_month": 7 })).await;
    assert_eq!(
        (status, body["error"]["code"].as_str()),
        (409, Some("LEAVE_YEAR_LOCKED"))
    );
    // Re-sending the current value is not a change.
    let (status, _) = patch_json(
        &admin,
        url.clone(),
        json!({ "leave_year_start_month": 4, "name": "Acme 2" }),
    )
    .await;
    assert_eq!(status, 200);
    // Other settings still change.
    let (status, _) = patch_json(&admin, url, json!({ "timezone": "UTC" })).await;
    assert_eq!(status, 200);
}

#[tokio::test]
async fn employees_cannot_read_or_change_org_settings() {
    let app = TestApp::spawn().await;
    let (org, _a, _admin) = org_with_admin(&app, "acme.test").await;
    let emp = create_user(&app, &org, NewUser::employee("e@acme.test")).await;
    let client = app.login_as(&emp).await;
    let (status, _) = get_json(&client, app.url("/api/v1/admin/org")).await;
    assert_eq!(status, 403);
    let (status, _) = patch_json(
        &client,
        app.url("/api/v1/admin/org"),
        json!({ "name": "x" }),
    )
    .await;
    assert_eq!(status, 403);
}

#[tokio::test]
async fn a_leave_request_locks_the_leave_year_start() {
    let app = TestApp::spawn().await;
    let (org, _admin_user, admin) = org_with_admin(&app, "acme.test").await;
    let cl = create_leave_type(&app, &org, NewLeaveType::all("CL", dec!(12))).await;
    let asha = create_user(&app, &org, NewUser::employee("asha@acme.test")).await;
    add_ledger_row(&app, &asha, cl, 2026, dec!(12), "allotment").await;
    let (status, out) = post_leave(
        &app,
        &app.login_as(&asha).await,
        &leave_body(cl, "2026-12-01", "2026-12-01"),
        &key(),
    )
    .await;
    assert_eq!(status, 201, "{out}");

    let (status, body) = patch_json(
        &admin,
        app.url("/api/v1/admin/org"),
        json!({ "leave_year_start_month": 4 }),
    )
    .await;
    assert_eq!(
        (status, body["error"]["code"].as_str()),
        (409, Some("LEAVE_YEAR_LOCKED"))
    );
}
