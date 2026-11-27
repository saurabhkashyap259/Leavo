//! T226 — cross-organization isolation (FR-048). Org A's admin (who may do everything inside
//! org A) calls every route of the OpenAPI document that takes a resource id, with org B's real
//! ids: each answers 404 and org B is unchanged. Every list or report route is then read as org
//! A and must not mention any of org B's ids, emails or names.
//!
//! The test reads the OpenAPI document, so a new route with a path parameter fails here until it
//! is added to the matrix or to the exemptions with a reason.
mod common;

use common::factories::*;
use common::*;
use rust_decimal_macros::dec;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use uuid::Uuid;

/// Routes with a path parameter that is not a tenant resource id.
const EXEMPT: &[(&str, &str)] = &[
    (
        "GET /api/v1/approvals/action/{token}",
        "signed, single-use email token; no session or org",
    ),
    (
        "GET /api/v1/auth/recovery/{link}",
        "signed one-time recovery link; no session",
    ),
    (
        "GET /api/v1/attachments/{id}/download",
        "HMAC-signed URL bound to org and file; tested in attachments_test",
    ),
    (
        "POST /api/v1/admin/import/{kind}",
        "`kind` is csv|keka, not an id",
    ),
];

struct OrgB {
    admin: User,
    employee: User,
    cl: Uuid,
    restricted: Uuid,
    group: Uuid,
    holiday: Uuid,
    domain: Uuid,
    leave: String,
    comp_off: String,
    credit: Uuid,
    attachment: Uuid,
}

async fn org_b(app: &TestApp) -> OrgB {
    let (org, admin, admin_c) = org_with_admin(app, "orgb.test").await;
    let cl = create_leave_type(app, &org, NewLeaveType::all("CL", dec!(12))).await;
    let restricted = create_leave_type(app, &org, NewLeaveType::restricted("ML", dec!(10))).await;
    create_leave_type(app, &org, NewLeaveType::comp_off()).await;
    let approver = create_user(
        app,
        &org,
        NewUser::employee("lead@orgb.test").role("approver"),
    )
    .await;
    let group = create_group(app, &org, "B-Team", &[&approver]).await;
    let employee = create_user(
        app,
        &org,
        NewUser::employee("worker@orgb.test").group(group),
    )
    .await;
    set_full_name(app, &employee, "Zed Orgbee").await;
    add_ledger_row(app, &employee, cl, 2026, dec!(12), "allotment").await;
    let emp_c = app.login_as(&employee).await;
    add_holiday(
        app,
        &org,
        "2026-12-24".parse().unwrap(),
        "B Founders Day",
        "mandatory",
    )
    .await;
    let holiday: Uuid = sqlx::query_scalar("SELECT id FROM holidays WHERE org_id = $1")
        .bind(org.id)
        .fetch_one(&app.db.owner)
        .await
        .unwrap();
    let domain: Uuid = sqlx::query_scalar("SELECT id FROM org_domains WHERE org_id = $1")
        .bind(org.id)
        .fetch_one(&app.db.owner)
        .await
        .unwrap();
    // A leave request with a document, and a comp-off credit plus a pending comp-off request.
    let form = reqwest::multipart::Form::new().part(
        "file",
        reqwest::multipart::Part::bytes(b"%PDF-1.4 b".to_vec())
            .file_name("b.pdf")
            .mime_str("application/pdf")
            .unwrap(),
    );
    let up: Value = emp_c
        .post(app.url("/api/v1/attachments"))
        .multipart(form)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let attachment: Uuid = up["id"].as_str().unwrap().parse().unwrap();
    let mut body = leave_body(cl, "2026-11-30", "2026-12-01");
    body["attachment_id"] = json!(attachment);
    body["reason"] = json!("B secret reason");
    let leave = submit(app, &emp_c, &body).await;
    earn_comp_off(app, &emp_c, &admin_c, &[("2026-11-21", 1.0)]).await;
    let credit: Uuid = sqlx::query_scalar("SELECT id FROM comp_off_credits WHERE org_id = $1")
        .bind(org.id)
        .fetch_one(&app.db.owner)
        .await
        .unwrap();
    let (s, out) = post_comp_off(app, &emp_c, &comp_off_body(&[("2026-11-22", 1.0)])).await;
    assert_eq!(s, 201, "{out}");
    let comp_off = out["id"].as_str().unwrap().to_string();
    OrgB {
        admin,
        employee,
        cl,
        restricted,
        group,
        holiday,
        domain,
        leave,
        comp_off,
        credit,
        attachment,
    }
}

/// A fingerprint of org B's data that a cross-org write must not change.
async fn snapshot(app: &TestApp) -> Value {
    let q = |sql: &'static str| {
        let pool = app.db.owner.clone();
        async move {
            sqlx::query_scalar::<_, Option<String>>(sql)
                .fetch_one(&pool)
                .await
                .unwrap()
        }
    };
    json!({
        "users": q("SELECT string_agg(u.email || u.role || u.status || u.full_name || coalesce(u.approver_group_id::text,''), ',' ORDER BY u.email) FROM users u JOIN org_domains d ON d.org_id = u.org_id WHERE d.domain = 'orgb.test'").await,
        "ledger": q("SELECT count(*)::text || sum(delta)::text FROM balance_ledger l JOIN users u ON u.id = l.user_id WHERE u.email LIKE '%@orgb.test'").await,
        "requests": q("SELECT string_agg(r.status::text || r.version, ',' ORDER BY r.id) FROM leave_requests r JOIN users u ON u.id = r.user_id WHERE u.email LIKE '%@orgb.test'").await,
        "comp_offs": q("SELECT string_agg(r.status::text || r.version, ',' ORDER BY r.id) FROM comp_off_requests r JOIN users u ON u.id = r.user_id WHERE u.email LIKE '%@orgb.test'").await,
        "credits": q("SELECT string_agg(coalesce(revoked_at::text, '-'), ',') FROM comp_off_credits c JOIN users u ON u.id = c.user_id WHERE u.email LIKE '%@orgb.test'").await,
        "holidays": q("SELECT string_agg(h.name || h.kind, ',') FROM holidays h JOIN org_domains d ON d.org_id = h.org_id WHERE d.domain = 'orgb.test'").await,
        "groups": q("SELECT string_agg(g.name, ',') FROM approver_groups g JOIN org_domains d ON d.org_id = g.org_id WHERE d.domain = 'orgb.test'").await,
        "types": q("SELECT string_agg(t.code || t.name || t.active, ',' ORDER BY t.code) FROM leave_types t JOIN org_domains d ON d.org_id = t.org_id WHERE d.domain = 'orgb.test'").await,
        "grants": q("SELECT count(*)::text FROM leave_type_grants g JOIN users u ON u.id = g.user_id WHERE u.email LIKE '%@orgb.test'").await,
        "domains": q("SELECT count(*)::text FROM org_domains WHERE domain = 'orgb.test'").await,
        "sessions": q("SELECT count(*)::text FROM sessions s JOIN users u ON u.id = s.user_id WHERE u.email LIKE '%@orgb.test' AND s.revoked_at IS NULL").await,
    })
}

async fn call(
    app: &TestApp,
    client: &reqwest::Client,
    method: &str,
    path: &str,
    body: Option<Value>,
) -> (u16, String) {
    let url = app.url(path);
    let req = match method {
        "GET" => client.get(url),
        "POST" => client.post(url),
        "PATCH" => client.patch(url),
        "DELETE" => client.delete(url),
        other => panic!("{other}"),
    };
    let req = req.header("Idempotency-Key", key());
    let req = match body {
        Some(b) => req.json(&b),
        None => req,
    };
    let res = req.send().await.unwrap();
    let status = res.status().as_u16();
    (status, res.text().await.unwrap())
}

#[tokio::test]
async fn org_a_cannot_reach_org_b() {
    let app = TestApp::spawn().await;
    let (org_a, _, a) = org_with_admin(&app, "orga.test").await;
    let a_type = create_leave_type(&app, &org_a, NewLeaveType::restricted("RA", dec!(5))).await;
    let b = org_b(&app).await;
    let before = snapshot(&app).await;

    let (bu, be, lv, co) = (b.admin.id, b.employee.id, &b.leave, &b.comp_off);
    // (operation as in the OpenAPI document, concrete path, body)
    let matrix: Vec<(&str, String, Option<Value>)> = vec![
        (
            "DELETE /api/v1/admin/approver-groups/{id}",
            format!("/api/v1/admin/approver-groups/{}", b.group),
            None,
        ),
        (
            "PATCH /api/v1/admin/approver-groups/{id}",
            format!("/api/v1/admin/approver-groups/{}", b.group),
            Some(json!({ "name": "Mine now" })),
        ),
        (
            "POST /api/v1/admin/comp-off-credits/{id}/revoke",
            format!("/api/v1/admin/comp-off-credits/{}/revoke", b.credit),
            Some(json!({ "reason": "x" })),
        ),
        (
            "DELETE /api/v1/admin/holidays/{id}",
            format!("/api/v1/admin/holidays/{}", b.holiday),
            None,
        ),
        (
            "PATCH /api/v1/admin/holidays/{id}",
            format!("/api/v1/admin/holidays/{}", b.holiday),
            Some(json!({ "kind": "optional" })),
        ),
        (
            "PATCH /api/v1/admin/leave-types/{id}",
            format!("/api/v1/admin/leave-types/{}", b.cl),
            Some(json!({ "name": "Hacked" })),
        ),
        (
            "GET /api/v1/admin/leave-types/{id}/grants",
            format!("/api/v1/admin/leave-types/{}/grants", b.restricted),
            None,
        ),
        (
            "POST /api/v1/admin/leave-types/{id}/grants",
            format!("/api/v1/admin/leave-types/{}/grants", b.restricted),
            Some(json!({ "user_id": be })),
        ),
        (
            "POST /api/v1/admin/leave-types/{id}/grants",
            format!("/api/v1/admin/leave-types/{a_type}/grants"),
            Some(json!({ "user_id": be })),
        ),
        (
            "DELETE /api/v1/admin/leave-types/{id}/grants/{user_id}",
            format!("/api/v1/admin/leave-types/{}/grants/{be}", b.restricted),
            None,
        ),
        (
            "DELETE /api/v1/admin/org/domains/{id}",
            format!("/api/v1/admin/org/domains/{}", b.domain),
            None,
        ),
        (
            "GET /api/v1/admin/users/{id}",
            format!("/api/v1/admin/users/{be}"),
            None,
        ),
        (
            "PATCH /api/v1/admin/users/{id}",
            format!("/api/v1/admin/users/{bu}"),
            Some(json!({ "status": "deactivated" })),
        ),
        (
            "POST /api/v1/admin/users/{id}/adjustments",
            format!("/api/v1/admin/users/{be}/adjustments"),
            Some(json!({ "leave_type_id": b.cl, "leave_year": 2026, "delta": 5, "note": "x" })),
        ),
        (
            "POST /api/v1/admin/users/{id}/anonymise",
            format!("/api/v1/admin/users/{be}/anonymise"),
            Some(json!({})),
        ),
        (
            "GET /api/v1/admin/users/{id}/balances",
            format!("/api/v1/admin/users/{be}/balances"),
            None,
        ),
        (
            "POST /api/v1/admin/users/{id}/export",
            format!("/api/v1/admin/users/{be}/export"),
            Some(json!({})),
        ),
        (
            "POST /api/v1/approvals/{kind}/{id}/decide",
            format!("/api/v1/approvals/leave/{lv}/decide"),
            Some(json!({ "decision": "approve", "version": 1 })),
        ),
        (
            "POST /api/v1/approvals/{kind}/{id}/decide",
            format!("/api/v1/approvals/comp_off/{co}/decide"),
            Some(json!({ "decision": "approve", "version": 1 })),
        ),
        (
            "POST /api/v1/approvals/{kind}/{id}/decide",
            format!("/api/v1/approvals/cancel/{lv}/decide"),
            Some(json!({ "decision": "approve", "version": 1 })),
        ),
        (
            "GET /api/v1/attachments/{id}/url",
            format!("/api/v1/attachments/{}/url", b.attachment),
            None,
        ),
        (
            "POST /api/v1/comp-offs/{id}/cancel",
            format!("/api/v1/comp-offs/{co}/cancel"),
            Some(json!({})),
        ),
        (
            "GET /api/v1/leaves/{id}",
            format!("/api/v1/leaves/{lv}"),
            None,
        ),
        (
            "POST /api/v1/leaves/{id}/cancel",
            format!("/api/v1/leaves/{lv}/cancel"),
            Some(json!({})),
        ),
    ];

    // Every operation with a path parameter is covered or exempt.
    use utoipa::OpenApi;
    let doc = serde_json::to_value(leavo_api::openapi::ApiDoc::openapi()).unwrap();
    let mut uncovered = Vec::new();
    for (path, ops) in doc["paths"].as_object().unwrap() {
        if !path.contains('{') {
            continue;
        }
        for (method, _) in ops.as_object().unwrap() {
            let op = format!("{} {path}", method.to_uppercase());
            let known = matrix.iter().any(|m| m.0 == op) || EXEMPT.iter().any(|e| e.0 == op);
            if !known {
                uncovered.push(op);
            }
        }
    }
    assert!(
        uncovered.is_empty(),
        "add these to the cross-org matrix or EXEMPT: {uncovered:#?}"
    );

    let mut wrong = Vec::new();
    for (op, path, body) in &matrix {
        let (status, text) =
            call(&app, &a, op.split(' ').next().unwrap(), path, body.clone()).await;
        let leaks =
            text.contains("orgb.test") || text.contains("Orgbee") || text.contains("B secret");
        if status != 404 || leaks {
            wrong.push(format!(
                "{op} ({path}) → {status} {}",
                text.chars().take(160).collect::<String>()
            ));
        }
    }
    assert!(
        wrong.is_empty(),
        "cross-org access not refused with 404:\n{}",
        wrong.join("\n")
    );
    assert_eq!(snapshot(&app).await, before, "org B changed");
}

#[tokio::test]
async fn lists_and_reports_never_show_org_b() {
    let app = TestApp::spawn().await;
    let (org_a, _, a) = org_with_admin(&app, "orga.test").await;
    let cl = create_leave_type(&app, &org_a, NewLeaveType::all("CL", dec!(12))).await;
    let _ = cl;
    let b = org_b(&app).await;
    let markers: BTreeMap<&str, String> = BTreeMap::from([
        ("domain", "orgb.test".to_string()),
        ("name", "Orgbee".to_string()),
        ("reason", "B secret".to_string()),
        ("holiday", "B Founders Day".to_string()),
        ("group", "B-Team".to_string()),
        ("employee", b.employee.id.to_string()),
        ("leave", b.leave.clone()),
        ("comp_off", b.comp_off.clone()),
        ("credit", b.credit.to_string()),
    ]);
    let reads = [
        "/api/v1/admin/approver-groups",
        "/api/v1/admin/audit-log",
        "/api/v1/admin/carry-forward/preview?year=2026",
        "/api/v1/admin/carry-forward/preview.csv?year=2026",
        "/api/v1/admin/dashboard",
        "/api/v1/admin/instance",
        "/api/v1/admin/leave-types",
        "/api/v1/admin/org",
        "/api/v1/admin/org/domains",
        "/api/v1/admin/reports/leaves?group_by=user&year=2026",
        "/api/v1/admin/reports/leaves.csv?group_by=user&year=2026",
        "/api/v1/admin/users",
        "/api/v1/approvals",
        "/api/v1/approvals?kind=comp_off",
        "/api/v1/approvals?kind=cancel",
        "/api/v1/away?range=month",
        "/api/v1/comp-offs",
        &format!("/api/v1/comp-offs?user={}", b.employee.id),
        "/api/v1/holidays",
        "/api/v1/me",
        "/api/v1/me/balances",
        "/api/v1/me/export",
        "/api/v1/me/leaves",
        "/api/v1/me/leaves.csv",
        "/api/v1/team/calendar?month=2026-11",
        "/api/v1/users/search?q=or",
        "/api/v1/users/search?q=Zed",
    ];
    let mut leaks = Vec::new();
    for path in reads {
        let (status, text) = call(&app, &a, "GET", path, None).await;
        for (what, m) in &markers {
            if text.contains(m.as_str()) {
                leaks.push(format!("{path} ({status}) shows org B's {what}"));
            }
        }
    }
    assert!(leaks.is_empty(), "{}", leaks.join("\n"));
}
