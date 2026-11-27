//! T176 — manual balance adjustments and the balance history (FR-040, FR-041).
mod common;

use common::factories::*;
use common::*;
use rust_decimal_macros::dec;
use serde_json::json;

#[tokio::test]
async fn adjustments_need_a_note_and_show_in_the_ledger() {
    let app = TestApp::spawn().await;
    let (org, admin_user, admin) = org_with_admin(&app, "acme.test").await;
    set_full_name(&app, &admin_user, "Ada Admin").await;
    let cl = create_leave_type(&app, &org, NewLeaveType::all("CL", dec!(12))).await;
    let asha = create_user(&app, &org, NewUser::employee("asha@acme.test")).await;
    add_ledger_row(&app, &asha, cl, 2026, dec!(12), "allotment").await;
    let url = app.url(&format!("/api/v1/admin/users/{}/adjustments", asha.id));

    for (body, code, field) in [
        (
            json!({ "leave_type_id": cl, "leave_year": 2026, "delta": 2 }),
            "NOTE_REQUIRED",
            None,
        ),
        (
            json!({ "leave_type_id": cl, "leave_year": 2026, "delta": 2, "note": "  " }),
            "NOTE_REQUIRED",
            None,
        ),
        (
            json!({ "leave_type_id": cl, "leave_year": 2026, "delta": 0, "note": "x" }),
            "VALIDATION",
            Some("delta"),
        ),
        (
            json!({ "leave_type_id": cl, "leave_year": 2026, "delta": 0.3, "note": "x" }),
            "VALIDATION",
            Some("delta"),
        ),
        (
            json!({ "leave_type_id": cl, "leave_year": 1999, "delta": 1, "note": "x" }),
            "VALIDATION",
            Some("leave_year"),
        ),
        (
            json!({ "leave_type_id": uuid::Uuid::nil(), "leave_year": 2026, "delta": 1, "note": "x" }),
            "VALIDATION",
            Some("leave_type_id"),
        ),
    ] {
        let (status, out) = post_json(&admin, url.clone(), body.clone()).await;
        assert_eq!(status, 422, "{body}: {out}");
        assert_eq!(out["error"]["code"], code, "{body}");
        if let Some(f) = field {
            assert_eq!(out["error"]["details"]["field"], f);
        }
    }

    let (status, out) = post_json(
        &admin,
        url.clone(),
        json!({ "leave_type_id": cl, "leave_year": 2026, "delta": -1.5, "note": "Unrecorded leave in March" }),
    )
    .await;
    assert_eq!(status, 201, "{out}");
    assert_eq!(out["reason"], "manual_adjustment");
    assert_eq!(out["delta"], -1.5);
    assert_eq!(out["created_by"]["full_name"], "Ada Admin");

    let (status, out) = get_json(
        &admin,
        app.url(&format!(
            "/api/v1/admin/users/{}/balances?year=2026",
            asha.id
        )),
    )
    .await;
    assert_eq!(status, 200, "{out}");
    let card = out["balances"]
        .as_array()
        .unwrap()
        .iter()
        .find(|b| b["leave_type"]["code"] == "CL")
        .unwrap();
    assert_eq!(card["adjusted"], -1.5);
    assert_eq!(card["available"], 10.5);
    let ledger = out["ledger"].as_array().unwrap();
    assert_eq!(ledger.len(), 2);
    let adj = &ledger[1];
    assert_eq!(adj["reason"], "manual_adjustment");
    assert_eq!(adj["note"], "Unrecorded leave in March");
    assert_eq!(adj["leave_type"]["code"], "CL");
    assert_eq!(adj["created_by"]["full_name"], "Ada Admin");
    assert!(adj["created_at"].is_string());
    // The employee sees the new balance too.
    let card = balance_card(&app, &app.login_as(&asha).await, "CL", None).await;
    assert_eq!(card["available"], 10.5);

    let asha_client = app.login_as(&asha).await;
    let (status, _) = post_json(
        &asha_client,
        url,
        json!({ "leave_type_id": cl, "leave_year": 2026, "delta": 5, "note": "mine" }),
    )
    .await;
    assert_eq!(status, 403);
}
