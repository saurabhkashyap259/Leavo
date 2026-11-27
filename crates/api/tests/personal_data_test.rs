//! T180 — personal data export and anonymisation (FR-045).
mod common;

use common::factories::*;
use common::*;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use serde_json::{json, Value};
use uuid::Uuid;

struct World {
    app: TestApp,
    admin: reqwest::Client,
    asha: User,
    asha_c: reqwest::Client,
    cl: Uuid,
    request: String,
    attachment_key: String,
}

async fn world() -> World {
    let app = TestApp::spawn().await;
    let (org, _, admin) = org_with_admin(&app, "acme.test").await;
    let cl = create_leave_type(&app, &org, NewLeaveType::all("CL", dec!(12))).await;
    create_leave_type(&app, &org, NewLeaveType::comp_off()).await;
    let asha = create_user(&app, &org, NewUser::employee("asha@acme.test")).await;
    set_full_name(&app, &asha, "Asha Rao").await;
    add_ledger_row(&app, &asha, cl, 2026, dec!(12), "allotment").await;
    let asha_c = app.login_as(&asha).await;

    // A request with a reason and a document, approved; and a comp-off request.
    let form = reqwest::multipart::Form::new().part(
        "file",
        reqwest::multipart::Part::bytes(b"%PDF-1.4 note".to_vec())
            .file_name("note.pdf")
            .mime_str("application/pdf")
            .unwrap(),
    );
    let up: Value = asha_c
        .post(app.url("/api/v1/attachments"))
        .multipart(form)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let mut body = leave_body(cl, "2026-11-30", "2026-12-01");
    body["reason"] = json!("Asha Rao's sister's wedding");
    body["attachment_id"] = up["id"].clone();
    let request = submit(&app, &asha_c, &body).await;
    let (s, out) = decide(&app, &admin, &request, approve()).await;
    assert_eq!(s, 200, "{out}");
    let (s, out) = post_comp_off(&app, &asha_c, &comp_off_body(&[("2026-11-21", 1.0)])).await;
    assert_eq!(s, 201, "{out}");
    let attachment_key: String = sqlx::query_scalar("SELECT storage_key FROM attachments")
        .fetch_one(&app.db.owner)
        .await
        .unwrap();
    World {
        app,
        admin,
        asha,
        asha_c,
        cl,
        request,
        attachment_key,
    }
}

fn check_export(out: &Value) {
    assert_eq!(out["profile"]["email"], "asha@acme.test");
    assert_eq!(out["profile"]["full_name"], "Asha Rao");
    assert_eq!(
        out["leave_requests"][0]["reason"],
        "Asha Rao's sister's wedding"
    );
    assert_eq!(out["leave_requests"][0]["status"], "approved");
    assert_eq!(out["comp_off_requests"][0]["reason"], "Release weekend");
    let ledger = out["ledger"].as_array().unwrap();
    assert_eq!(ledger.len(), 2);
    assert!(ledger
        .iter()
        .any(|r| r["reason"] == "leave_taken" && r["delta"] == -2.0));
    assert!(out["generated_at"].is_string());
}

#[tokio::test]
async fn employees_download_their_own_data_and_admins_anyones() {
    let w = world().await;
    let res = w
        .asha_c
        .get(w.app.url("/api/v1/me/export"))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 200);
    assert!(res.headers()["content-disposition"]
        .to_str()
        .unwrap()
        .contains("attachment"));
    check_export(&res.json().await.unwrap());

    let (status, out) = post_json(
        &w.admin,
        w.app
            .url(&format!("/api/v1/admin/users/{}/export", w.asha.id)),
        json!({}),
    )
    .await;
    assert_eq!(status, 200, "{out}");
    check_export(&out);
    let (status, _) = post_json(
        &w.asha_c,
        w.app
            .url(&format!("/api/v1/admin/users/{}/export", w.asha.id)),
        json!({}),
    )
    .await;
    assert_eq!(status, 403);
}

#[tokio::test]
async fn anonymising_keeps_the_numbers_and_removes_the_person() {
    let w = world().await;
    let url = w
        .app
        .url(&format!("/api/v1/admin/users/{}/anonymise", w.asha.id));
    // Only deactivated users.
    let (status, out) = post_json(&w.admin, url.clone(), json!({})).await;
    assert_eq!(status, 422, "{out}");
    assert_eq!(out["error"]["details"]["field"], "status");
    patch_json(
        &w.admin,
        w.app.url(&format!("/api/v1/admin/users/{}", w.asha.id)),
        json!({ "status": "deactivated" }),
    )
    .await;
    let total_before: Decimal =
        sqlx::query_scalar("SELECT SUM(delta) FROM balance_ledger WHERE user_id = $1")
            .bind(w.asha.id)
            .fetch_one(&w.app.db.owner)
            .await
            .unwrap();

    let (status, out) = post_json(&w.admin, url.clone(), json!({})).await;
    assert_eq!(status, 200, "{out}");
    let (name, email, anonymised): (String, String, bool) = sqlx::query_as(
        "SELECT full_name, email::text, anonymised_at IS NOT NULL FROM users WHERE id = $1",
    )
    .bind(w.asha.id)
    .fetch_one(&w.app.db.owner)
    .await
    .unwrap();
    assert!(anonymised);
    assert!(
        !name.contains("Asha") && !email.contains("asha"),
        "{name} {email}"
    );

    // Reasons gone; dates, days and ledger kept.
    let (reason, days, from): (String, Decimal, chrono::NaiveDate) =
        sqlx::query_as("SELECT reason, days, from_date FROM leave_requests WHERE id = $1::uuid")
            .bind(&w.request)
            .fetch_one(&w.app.db.owner)
            .await
            .unwrap();
    assert!(!reason.contains("wedding"));
    assert_eq!(
        (days, from.to_string()),
        (dec!(2), "2026-11-30".to_string())
    );
    let co_reason: String = sqlx::query_scalar("SELECT reason FROM comp_off_requests")
        .fetch_one(&w.app.db.owner)
        .await
        .unwrap();
    assert!(!co_reason.contains("Release"));
    let total_after: Decimal =
        sqlx::query_scalar("SELECT SUM(delta) FROM balance_ledger WHERE user_id = $1")
            .bind(w.asha.id)
            .fetch_one(&w.app.db.owner)
            .await
            .unwrap();
    assert_eq!(total_after, total_before);
    assert_eq!(ledger_rows(&w.app, w.asha.id, w.cl).await.len(), 2);

    // Attachments deleted, rows and stored objects.
    let attachments: i64 = sqlx::query_scalar("SELECT count(*) FROM attachments")
        .fetch_one(&w.app.db.owner)
        .await
        .unwrap();
    assert_eq!(attachments, 0);
    assert!(w.app.state.storage.get(&w.attachment_key).await.is_err());

    // Nothing in the audit log names her or quotes her reasons any more.
    let audit: Vec<String> = sqlx::query_scalar(
        "SELECT COALESCE(before::text, '') || COALESCE(after::text, '') FROM audit_log",
    )
    .fetch_all(&w.app.db.owner)
    .await
    .unwrap();
    for row in &audit {
        assert!(!row.contains("asha@acme.test"), "{row}");
        assert!(!row.contains("Asha Rao"), "{row}");
        assert!(!row.contains("wedding"), "{row}");
        assert!(!row.contains("Release weekend"), "{row}");
    }
    let logged: i64 =
        sqlx::query_scalar("SELECT count(*) FROM audit_log WHERE action = 'user.anonymise'")
            .fetch_one(&w.app.db.owner)
            .await
            .unwrap();
    assert_eq!(logged, 1);

    // Once is enough.
    let (status, _) = post_json(&w.admin, url, json!({})).await;
    assert_eq!(status, 409);
}
