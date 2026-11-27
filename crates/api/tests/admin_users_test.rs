//! T174 — managing people (FR-006, FR-013, FR-014, FR-042, FR-064).
mod common;

use common::factories::*;
use common::*;
use rust_decimal_macros::dec;
use serde_json::{json, Value};
use uuid::Uuid;

struct World {
    app: TestApp,
    org: Org,
    admin_user: User,
    admin: reqwest::Client,
    cl: Uuid,
}

async fn world() -> World {
    let app = TestApp::spawn().await;
    let (org, admin_user, admin) = org_with_admin(&app, "acme.test").await;
    let cl = create_leave_type(&app, &org, NewLeaveType::all("CL", dec!(12))).await;
    World {
        app,
        org,
        admin_user,
        admin,
        cl,
    }
}

async fn invite(w: &World, body: Value) -> (u16, Value) {
    post_json(&w.admin, w.app.url("/api/v1/admin/users"), body).await
}

async fn patch_user(w: &World, client: &reqwest::Client, id: &str, body: Value) -> (u16, Value) {
    patch_json(
        client,
        w.app.url(&format!("/api/v1/admin/users/{id}")),
        body,
    )
    .await
}

#[tokio::test]
async fn inviting_creates_an_invited_user_with_allotment_and_an_email() {
    let w = world().await;
    let eng = create_group(&w.app, &w.org, "Eng", &[]).await;
    w.app.email.take();
    let (status, out) = invite(
        &w,
        json!({
            "email": "Asha@Acme.test", "full_name": "Asha Rao", "employee_code": "E-7",
            "department": "Platform", "date_of_joining": "2026-01-01", "role": "employee",
            "approver_group_id": eng,
        }),
    )
    .await;
    assert_eq!(status, 201, "{out}");
    assert_eq!(out["email"], "asha@acme.test");
    assert_eq!(out["status"], "invited");
    assert_eq!(out["role"], "employee");
    assert_eq!(out["approver_group"]["name"], "Eng");
    assert_eq!(out["employee_code"], "E-7");
    let (status, one) = get_json(
        &w.admin,
        w.app.url(&format!(
            "/api/v1/admin/users/{}",
            out["id"].as_str().unwrap()
        )),
    )
    .await;
    assert_eq!(status, 200);
    assert_eq!(one, out);
    let id: Uuid = out["id"].as_str().unwrap().parse().unwrap();
    // The year's quota is allotted at invite (FR-013).
    assert_eq!(
        ledger_rows(&w.app, id, w.cl).await,
        [("allotment".into(), dec!(12), 2026, Some("user".into()))]
    );
    w.app.run_jobs().await;
    let sent = w.app.email.take();
    let mail = sent
        .iter()
        .find(|m| m.to == "asha@acme.test")
        .expect("invite email");
    assert!(
        mail.subject.contains("Acme") || mail.subject.contains("acme"),
        "{}",
        mail.subject
    );
    assert!(
        mail.text.contains("http://leavo.test/login"),
        "{}",
        mail.text
    );

    // Validation.
    for (body, code, field) in [
        (
            json!({ "email": "asha@acme.test", "full_name": "Again" }),
            "VALIDATION",
            Some("email"),
        ),
        (
            json!({ "email": "x@gmail.com", "full_name": "X" }),
            "DOMAIN_NOT_REGISTERED",
            None,
        ),
        (
            json!({ "email": "bad", "full_name": "X" }),
            "VALIDATION",
            Some("email"),
        ),
        (
            json!({ "email": "b@acme.test", "full_name": " " }),
            "VALIDATION",
            Some("full_name"),
        ),
        (
            json!({ "email": "b@acme.test", "full_name": "B", "role": "boss" }),
            "VALIDATION",
            Some("role"),
        ),
        (
            json!({ "email": "b@acme.test", "full_name": "B", "employee_code": "E-7" }),
            "VALIDATION",
            Some("employee_code"),
        ),
        (
            json!({ "email": "b@acme.test", "full_name": "B", "approver_group_id": Uuid::nil() }),
            "VALIDATION",
            Some("approver_group_id"),
        ),
    ] {
        let (status, out) = invite(&w, body.clone()).await;
        assert_eq!(status, 422, "{body}: {out}");
        assert_eq!(out["error"]["code"], code, "{body}");
        if let Some(f) = field {
            assert_eq!(out["error"]["details"]["field"], f, "{body}");
        }
    }

    // Employees cannot manage people.
    let emp = create_user(&w.app, &w.org, NewUser::employee("ravi@acme.test")).await;
    let ravi = w.app.login_as(&emp).await;
    let (status, _) = post_json(
        &ravi,
        w.app.url("/api/v1/admin/users"),
        json!({ "email": "c@acme.test", "full_name": "C" }),
    )
    .await;
    assert_eq!(status, 403);
    let (status, _) = get_json(&ravi, w.app.url("/api/v1/admin/users")).await;
    assert_eq!(status, 403);
}

#[tokio::test]
async fn the_list_filters_and_pages() {
    let w = world().await;
    let eng = create_group(&w.app, &w.org, "Eng", &[]).await;
    for (email, role, group) in [
        ("asha@acme.test", "employee", Some(eng)),
        ("ravi@acme.test", "employee", None),
        ("priya@acme.test", "approver", None),
    ] {
        let mut u = NewUser::employee(email).role(role);
        if let Some(g) = group {
            u = u.group(g);
        }
        create_user(&w.app, &w.org, u).await;
    }
    let get = |q: &str| {
        let url = w.app.url(&format!("/api/v1/admin/users?{q}"));
        let client = w.admin.clone();
        async move { get_json(&client, url).await }
    };
    let emails = |v: &Value| -> Vec<String> {
        v["items"]
            .as_array()
            .unwrap()
            .iter()
            .map(|u| u["email"].as_str().unwrap().into())
            .collect()
    };
    let (status, all) = get("").await;
    assert_eq!(status, 200, "{all}");
    assert_eq!(emails(&all).len(), 4);
    assert_eq!(emails(&get("role=approver").await.1), ["priya@acme.test"]);
    assert_eq!(
        emails(&get(&format!("group={eng}")).await.1),
        ["asha@acme.test"]
    );
    assert_eq!(emails(&get("q=rav").await.1), ["ravi@acme.test"]);
    assert_eq!(emails(&get("status=invited").await.1), Vec::<String>::new());
    let (_, first) = get("limit=2").await;
    assert_eq!(emails(&first).len(), 2);
    let cursor = first["next_cursor"].as_str().unwrap().to_string();
    let (_, second) = get(&format!("limit=2&cursor={cursor}")).await;
    assert_eq!(emails(&second).len(), 2);
    assert!(second["next_cursor"].is_null());
    let (status, out) = get("role=boss").await;
    assert_eq!(status, 422, "{out}");
}

const CSV: &str = "email,full_name,employee_code,department,date_of_joining,role,approver_group\n\
asha@acme.test,Asha Rao,E-1,Platform,2026-01-01,employee,Eng\n\
priya@acme.test,Priya Shah,E-2,Platform,,approver,\n\
admin@acme.test,Already Here,,,,,\n";

async fn import(w: &World, csv: &str, dry_run: bool) -> (u16, Value) {
    let form = reqwest::multipart::Form::new().part(
        "file",
        reqwest::multipart::Part::bytes(csv.as_bytes().to_vec()).file_name("people.csv"),
    );
    let res = w
        .admin
        .post(
            w.app
                .url(&format!("/api/v1/admin/users/import?dry_run={dry_run}")),
        )
        .multipart(form)
        .send()
        .await
        .unwrap();
    let status = res.status().as_u16();
    (status, res.json().await.unwrap_or(Value::Null))
}

#[tokio::test]
async fn csv_invites_preview_then_apply() {
    let w = world().await;
    let (status, out) = import(&w, CSV, true).await;
    assert_eq!(status, 200, "{out}");
    assert_eq!(out["dry_run"], true);
    let creating: Vec<&str> = out["to_create"]
        .as_array()
        .unwrap()
        .iter()
        .map(|u| u["email"].as_str().unwrap())
        .collect();
    assert_eq!(creating, ["asha@acme.test", "priya@acme.test"]);
    assert_eq!(
        out["skipped"],
        json!([{ "row": 4, "email": "admin@acme.test", "reason": "exists" }])
    );
    assert_eq!(
        out["warnings"],
        json!([{ "row": 2, "code": "GROUP_WILL_BE_CREATED", "message": "Group \"Eng\" will be created." }])
    );
    assert_eq!(out["errors"], json!([]));
    let (_, list) = get_json(&w.admin, w.app.url("/api/v1/admin/users")).await;
    assert_eq!(
        list["items"].as_array().unwrap().len(),
        1,
        "a dry run writes nothing"
    );

    let (status, out) = import(&w, CSV, false).await;
    assert_eq!(status, 200, "{out}");
    let (_, list) = get_json(&w.admin, w.app.url("/api/v1/admin/users?q=asha")).await;
    let asha = &list["items"][0];
    assert_eq!(asha["status"], "invited");
    assert_eq!(asha["approver_group"]["name"], "Eng");
    assert_eq!(asha["date_of_joining"], "2026-01-01");

    let bad = "email,full_name,role\nx@gmail.com,X,\ny@acme.test,,\nz@acme.test,Z,boss\nq@acme.test,Q,\nq@acme.test,Q again,\n";
    let (_, out) = import(&w, bad, true).await;
    let codes: Vec<(u64, &str)> = out["errors"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| (e["row"].as_u64().unwrap(), e["code"].as_str().unwrap()))
        .collect();
    assert_eq!(
        codes,
        [
            (2, "UNKNOWN_DOMAIN"),
            (3, "MISSING_VALUE"),
            (4, "INVALID_ROLE"),
            (6, "DUPLICATE_ROW")
        ]
    );
    let (status, out) = import(&w, bad, false).await;
    assert_eq!(status, 422);
    assert_eq!(out["error"]["code"], "IMPORT_HAS_ERRORS");
}

#[tokio::test]
async fn roles_groups_and_the_last_admin() {
    let w = world().await;
    let eng = create_group(&w.app, &w.org, "Eng", &[]).await;
    let asha = create_user(&w.app, &w.org, NewUser::employee("asha@acme.test")).await;
    let id = asha.id.to_string();
    let (status, out) = patch_user(
        &w,
        &w.admin,
        &id,
        json!({ "role": "approver", "approver_group_id": eng, "department": "Platform" }),
    )
    .await;
    assert_eq!(status, 200, "{out}");
    assert_eq!(out["role"], "approver");
    assert_eq!(out["approver_group"]["id"], eng.to_string());
    let (_, out) = patch_user(&w, &w.admin, &id, json!({ "approver_group_id": null })).await;
    assert!(out["approver_group"].is_null());

    // The only admin can be neither demoted nor deactivated.
    let me = w.admin_user.id.to_string();
    for body in [
        json!({ "role": "approver" }),
        json!({ "status": "deactivated" }),
    ] {
        let (status, out) = patch_user(&w, &w.admin, &me, body.clone()).await;
        assert_eq!(status, 422, "{body}: {out}");
        assert_eq!(out["error"]["code"], "LAST_ADMIN");
    }
    // With a second admin, the first may step down.
    patch_user(&w, &w.admin, &id, json!({ "role": "admin" })).await;
    let (status, out) = patch_user(&w, &w.admin, &me, json!({ "role": "approver" })).await;
    assert_eq!(status, 200, "{out}");
    let (status, _) = patch_user(&w, &w.admin, &id, json!({ "status": "deactivated" })).await;
    assert_eq!(status, 403, "the caller is no longer an admin");

    let (status, out) =
        patch_user(&w, &w.app.login_as(&asha).await, &me, json!({ "nope": 1 })).await;
    assert_eq!(status, 422, "{out}");
    assert_eq!(out["error"]["details"]["field"], "nope");
}

#[tokio::test]
async fn deactivation_ends_sessions_at_once_and_keeps_history() {
    let w = world().await;
    let asha = create_user(&w.app, &w.org, NewUser::employee("asha@acme.test")).await;
    add_ledger_row(&w.app, &asha, w.cl, 2026, dec!(12), "allotment").await;
    let client = w.app.login_as(&asha).await;
    let (status, _) = get_json(&client, w.app.url("/api/v1/me")).await;
    assert_eq!(status, 200);
    let (status, out) = patch_user(
        &w,
        &w.admin,
        &asha.id.to_string(),
        json!({ "status": "deactivated" }),
    )
    .await;
    assert_eq!(status, 200, "{out}");
    assert_eq!(out["status"], "deactivated");
    let (status, _) = get_json(&client, w.app.url("/api/v1/me")).await;
    assert_eq!(status, 401);
    let revoked: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM sessions WHERE user_id = $1 AND revoked_at IS NULL",
    )
    .bind(asha.id)
    .fetch_one(&w.app.db.owner)
    .await
    .unwrap();
    assert_eq!(revoked, 0);
    assert_eq!(ledger_rows(&w.app, asha.id, w.cl).await.len(), 1);
    // Reactivated: can sign in again.
    let (_, out) = patch_user(
        &w,
        &w.admin,
        &asha.id.to_string(),
        json!({ "status": "active" }),
    )
    .await;
    assert_eq!(out["status"], "active");
}

#[tokio::test]
async fn only_admins_change_the_joining_date_and_nothing_is_reallotted() {
    let w = world().await;
    let asha = create_user(&w.app, &w.org, NewUser::employee("asha@acme.test")).await;
    add_ledger_row(&w.app, &asha, w.cl, 2026, dec!(12), "allotment").await;
    let (status, out) = patch_user(
        &w,
        &w.admin,
        &asha.id.to_string(),
        json!({ "date_of_joining": "2026-07-01" }),
    )
    .await;
    assert_eq!(status, 200, "{out}");
    assert_eq!(out["date_of_joining"], "2026-07-01");
    assert_eq!(
        ledger_rows(&w.app, asha.id, w.cl).await.len(),
        1,
        "no re-allotment (FR-064)"
    );
    let client = w.app.login_as(&asha).await;
    let (status, _) = patch_json(
        &client,
        w.app.url("/api/v1/me"),
        json!({ "date_of_joining": "2026-01-01" }),
    )
    .await;
    assert_eq!(status, 422);
}
