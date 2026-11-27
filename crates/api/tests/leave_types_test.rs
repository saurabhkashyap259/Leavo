//! T072 — leave type administration and mid-year quota changes (FR-011, FR-056).
mod common;

use chrono::{TimeZone, Utc};
use common::factories::*;
use common::*;
use rust_decimal_macros::dec;
use serde_json::json;

/// JSON equality where numbers compare by value (`3` == `3.0`; days are sent as floats).
fn same(a: &serde_json::Value, b: &serde_json::Value) -> bool {
    a == b || (a.is_number() && b.is_number() && a.as_f64() == b.as_f64())
}

fn full_type() -> serde_json::Value {
    json!({
        "code": "WFH", "name": "Study Leave", "color": "#14B8A6", "yearly_quota": 4,
        "allow_half_day": false, "allow_negative": true, "negative_limit": 2,
        "max_days_per_request": 3, "min_notice_days": 7, "max_backdate_days": 3,
        "attachment_required_above_days": 1.5, "availability": "restricted",
        "receives_carry_forward": false, "active": true
    })
}

#[tokio::test]
async fn admin_creates_lists_and_edits_every_field() {
    let app = TestApp::spawn().await;
    let (_org, _admin, admin) = org_with_admin(&app, "acme.test").await;
    let url = app.url("/api/v1/admin/leave-types");

    let (status, created) = post_json(&admin, url.clone(), full_type()).await;
    assert_eq!(status, 201, "{created}");
    for (k, v) in full_type().as_object().unwrap() {
        assert!(same(&created[k], v), "{k}: {} vs {v}", created[k]);
    }
    assert_eq!(created["is_comp_off"], false);
    assert_eq!(created["pending_yearly_quota"], serde_json::Value::Null);

    let id = created["id"].as_str().unwrap();
    let patch = json!({
        "name": "Study", "code": "STD", "color": "#000000", "allow_half_day": true,
        "allow_negative": false, "negative_limit": 0, "max_days_per_request": null,
        "min_notice_days": 0, "max_backdate_days": 0, "attachment_required_above_days": null,
        "availability": "all", "receives_carry_forward": true, "active": false
    });
    let (status, updated) = patch_json(
        &admin,
        app.url(&format!("/api/v1/admin/leave-types/{id}")),
        patch.clone(),
    )
    .await;
    assert_eq!(status, 200, "{updated}");
    for (k, v) in patch.as_object().unwrap() {
        assert!(same(&updated[k], v), "{k}: {} vs {v}", updated[k]);
    }

    let (status, list) = get_json(&admin, url).await;
    assert_eq!(status, 200);
    assert_eq!(list.as_array().unwrap().len(), 1);
    assert_eq!(list[0]["code"], "STD");
    assert_eq!(list[0]["granted_count"], 0);
}

#[tokio::test]
async fn create_validates_fields() {
    let app = TestApp::spawn().await;
    let (org, _admin, admin) = org_with_admin(&app, "acme.test").await;
    create_leave_type(&app, &org, NewLeaveType::all("CL", dec!(12))).await;
    let url = app.url("/api/v1/admin/leave-types");
    for (field, value) in [
        ("code", json!("cl")),
        ("code", json!("CL")),
        ("color", json!("blue")),
        ("yearly_quota", json!(2.3)),
        ("max_backdate_days", json!(31)),
        ("name", json!("")),
    ] {
        let mut body = full_type();
        body[field] = value.clone();
        let (status, res) = post_json(&admin, url.clone(), body).await;
        assert_eq!(status, 422, "{field}={value} → {res}");
        assert_eq!(res["error"]["details"]["field"], field, "{field}={value}");
    }
}

#[tokio::test]
async fn quota_change_requires_apply_from() {
    let app = TestApp::spawn().await;
    let (org, _admin, admin) = org_with_admin(&app, "acme.test").await;
    let cl = create_leave_type(&app, &org, NewLeaveType::all("CL", dec!(12))).await;
    let url = app.url(&format!("/api/v1/admin/leave-types/{cl}"));

    let (status, body) = patch_json(&admin, url.clone(), json!({ "yearly_quota": 18 })).await;
    assert_eq!(
        (status, body["error"]["code"].as_str()),
        (422, Some("QUOTA_APPLY_FROM_REQUIRED"))
    );
    // Unchanged quota needs no choice.
    let (status, _) =
        patch_json(&admin, url, json!({ "yearly_quota": 12, "name": "Casual" })).await;
    assert_eq!(status, 200);
}

#[tokio::test]
async fn apply_now_writes_a_quota_change_row_for_every_affected_employee() {
    let app = TestApp::spawn().await;
    // 10 July 2026 in Asia/Kolkata: July counts, 6 months remain → (18 − 12) × 6/12 = 3.
    app.clock
        .set(Utc.with_ymd_and_hms(2026, 7, 10, 6, 0, 0).unwrap());
    let (org, admin_user, admin) = org_with_admin(&app, "acme.test").await;
    let cl = create_leave_type(&app, &org, NewLeaveType::all("CL", dec!(12))).await;
    let a = create_user(&app, &org, NewUser::employee("a@acme.test")).await;
    let invited = create_user(
        &app,
        &org,
        NewUser::employee("i@acme.test").status("invited"),
    )
    .await;
    let gone = create_user(
        &app,
        &org,
        NewUser::employee("g@acme.test").status("deactivated"),
    )
    .await;

    let (status, body) = patch_json(
        &admin,
        app.url(&format!("/api/v1/admin/leave-types/{cl}")),
        json!({ "yearly_quota": 18, "apply_from": "now" }),
    )
    .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["yearly_quota"], 18.0);
    assert_eq!(body["pending_yearly_quota"], serde_json::Value::Null);

    for u in [&a, &invited, &admin_user] {
        assert_eq!(
            ledger_rows(&app, u.id, cl).await,
            [(
                "quota_change".to_string(),
                dec!(3.0),
                2026,
                Some("leave_type".to_string())
            )],
            "{}",
            u.email
        );
    }
    assert!(ledger_rows(&app, gone.id, cl).await.is_empty());
    let audit: i64 =
        sqlx::query_scalar("SELECT count(*) FROM audit_log WHERE action = 'policy.update'")
            .fetch_one(&app.db.owner)
            .await
            .unwrap();
    assert_eq!(audit, 1);
}

#[tokio::test]
async fn apply_now_on_a_restricted_type_affects_only_grantees() {
    let app = TestApp::spawn().await;
    app.clock
        .set(Utc.with_ymd_and_hms(2026, 7, 10, 6, 0, 0).unwrap());
    let (org, _admin_user, admin) = org_with_admin(&app, "acme.test").await;
    let pl = create_leave_type(&app, &org, NewLeaveType::restricted("PL", dec!(10))).await;
    let a = create_user(&app, &org, NewUser::employee("a@acme.test")).await;
    let b = create_user(&app, &org, NewUser::employee("b@acme.test")).await;
    let (status, _) = post_json(
        &admin,
        app.url(&format!("/api/v1/admin/leave-types/{pl}/grants")),
        json!({ "user_id": a.id }),
    )
    .await;
    assert_eq!(status, 201);

    let (status, _) = patch_json(
        &admin,
        app.url(&format!("/api/v1/admin/leave-types/{pl}")),
        json!({ "yearly_quota": 14, "apply_from": "now" }),
    )
    .await;
    assert_eq!(status, 200);
    let rows = ledger_rows(&app, a.id, pl).await;
    assert_eq!(rows.last().unwrap().0, "quota_change");
    assert_eq!(rows.last().unwrap().1, dec!(2.0));
    assert!(ledger_rows(&app, b.id, pl).await.is_empty());
}

#[tokio::test]
async fn apply_next_year_only_records_the_pending_quota() {
    let app = TestApp::spawn().await;
    let (org, admin_user, admin) = org_with_admin(&app, "acme.test").await;
    let cl = create_leave_type(&app, &org, NewLeaveType::all("CL", dec!(12))).await;
    let (status, body) = patch_json(
        &admin,
        app.url(&format!("/api/v1/admin/leave-types/{cl}")),
        json!({ "yearly_quota": 15, "apply_from": "next_year" }),
    )
    .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["yearly_quota"], 12.0);
    assert_eq!(body["pending_yearly_quota"], 15.0);
    assert!(ledger_rows(&app, admin_user.id, cl).await.is_empty());
}

#[tokio::test]
async fn comp_off_type_has_no_quota_and_employees_cannot_manage_types() {
    let app = TestApp::spawn().await;
    let (org, _admin, admin) = org_with_admin(&app, "acme.test").await;
    let co = create_leave_type(&app, &org, NewLeaveType::comp_off()).await;
    let (status, body) = patch_json(
        &admin,
        app.url(&format!("/api/v1/admin/leave-types/{co}")),
        json!({ "yearly_quota": 5, "apply_from": "now" }),
    )
    .await;
    assert_eq!(
        (status, body["error"]["details"]["field"].as_str()),
        (422, Some("yearly_quota"))
    );

    let emp = create_user(&app, &org, NewUser::employee("e@acme.test")).await;
    let client = app.login_as(&emp).await;
    let (status, _) = get_json(&client, app.url("/api/v1/admin/leave-types")).await;
    assert_eq!(status, 403);
    let (status, _) = post_json(&client, app.url("/api/v1/admin/leave-types"), full_type()).await;
    assert_eq!(status, 403);

    let (_o, _b, other_admin) = org_with_admin(&app, "beta.test").await;
    let (status, _) = patch_json(
        &other_admin,
        app.url(&format!("/api/v1/admin/leave-types/{co}")),
        json!({ "name": "Hijack" }),
    )
    .await;
    assert_eq!(status, 404, "cross-org");
}
