//! T175 — approver groups (FR-021): members must be approvers or admins; an employee belongs
//! to at most one group.
mod common;

use common::factories::*;
use common::*;
use serde_json::{json, Value};

#[tokio::test]
async fn groups_are_created_edited_and_deleted() {
    let app = TestApp::spawn().await;
    let (org, admin_user, admin) = org_with_admin(&app, "acme.test").await;
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
    let asha = create_user(&app, &org, NewUser::employee("asha@acme.test")).await;
    let url = app.url("/api/v1/admin/approver-groups");

    let (status, out) = post_json(
        &admin,
        url.clone(),
        json!({ "name": "Eng", "member_ids": [priya.id, admin_user.id] }),
    )
    .await;
    assert_eq!(status, 201, "{out}");
    let id = out["id"].as_str().unwrap().to_string();
    let mut members: Vec<String> = out["members"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["id"].as_str().unwrap().into())
        .collect();
    members.sort();
    let mut expected = vec![priya.id.to_string(), admin_user.id.to_string()];
    expected.sort();
    assert_eq!(members, expected);

    for (body, field) in [
        (json!({ "name": "Eng", "member_ids": [] }), "name"),
        (json!({ "name": " ", "member_ids": [] }), "name"),
        (
            json!({ "name": "Sales", "member_ids": [asha.id] }),
            "member_ids",
        ),
        (
            json!({ "name": "Sales", "member_ids": [uuid::Uuid::nil()] }),
            "member_ids",
        ),
    ] {
        let (status, out) = post_json(&admin, url.clone(), body.clone()).await;
        assert_eq!(status, 422, "{body}: {out}");
        assert_eq!(out["error"]["details"]["field"], field, "{body}");
    }

    // An employee joins one group; moving them leaves the first.
    let sales = post_json(
        &admin,
        url.clone(),
        json!({ "name": "Sales", "member_ids": [vik.id] }),
    )
    .await
    .1;
    let sales_id = sales["id"].as_str().unwrap().to_string();
    let user_url = app.url(&format!("/api/v1/admin/users/{}", asha.id));
    patch_json(&admin, user_url.clone(), json!({ "approver_group_id": id })).await;
    patch_json(&admin, user_url, json!({ "approver_group_id": sales_id })).await;
    let (status, list) = get_json(&admin, url.clone()).await;
    assert_eq!(status, 200, "{list}");
    let count = |name: &str| -> Value {
        list.as_array()
            .unwrap()
            .iter()
            .find(|g| g["name"] == name)
            .unwrap()["employees"]
            .clone()
    };
    assert_eq!(count("Eng"), 0);
    assert_eq!(count("Sales"), 1);

    let (status, out) = patch_json(
        &admin,
        app.url(&format!("/api/v1/admin/approver-groups/{id}")),
        json!({ "name": "Engineering", "member_ids": [priya.id] }),
    )
    .await;
    assert_eq!(status, 200, "{out}");
    assert_eq!(out["name"], "Engineering");
    assert_eq!(out["members"].as_array().unwrap().len(), 1);

    // Demoting a member to employee removes them from the groups they approve.
    patch_json(
        &admin,
        app.url(&format!("/api/v1/admin/users/{}", vik.id)),
        json!({ "role": "employee" }),
    )
    .await;
    let (_, list) = get_json(&admin, url.clone()).await;
    let sales = list
        .as_array()
        .unwrap()
        .iter()
        .find(|g| g["name"] == "Sales")
        .unwrap()
        .clone();
    assert_eq!(sales["members"], json!([]));

    let (status, _) = delete(
        &admin,
        app.url(&format!("/api/v1/admin/approver-groups/{sales_id}")),
    )
    .await;
    assert_eq!(status, 204);
    let group: Option<uuid::Uuid> =
        sqlx::query_scalar("SELECT approver_group_id FROM users WHERE id = $1")
            .bind(asha.id)
            .fetch_one(&app.db.owner)
            .await
            .unwrap();
    assert_eq!(group, None, "members of a deleted group have no group");

    let asha_client = app.login_as(&asha).await;
    let (status, _) = get_json(&asha_client, url).await;
    assert_eq!(status, 403);
}
