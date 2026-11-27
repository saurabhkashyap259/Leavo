//! T111 — approvals inbox (FR-022, FR-058): only eligible approvers and admins see a request,
//! oldest first, with the requester's balance and who else on the team is away.
mod common;

use chrono::Duration;
use common::factories::*;
use common::*;
use rust_decimal_macros::dec;
use serde_json::json;
use uuid::Uuid;

struct World {
    app: TestApp,
    org: Org,
    admin: reqwest::Client,
    priya: reqwest::Client,
    vik: reqwest::Client,
    asha: User,
    ravi: User,
    sam: User,
    sales: Uuid,
    cl: Uuid,
}

/// Eng: approver Priya; employees Asha and Ravi. Sales: approver Vik; employee Sam, who is in
/// Asha's department. Everyone has 12 days of CL.
async fn world() -> World {
    let app = TestApp::spawn().await;
    let (org, _admin_user, admin) = org_with_admin(&app, "acme.test").await;
    let cl = create_leave_type(&app, &org, NewLeaveType::all("CL", dec!(12))).await;
    sqlx::query("UPDATE leave_types SET name = 'Casual Leave' WHERE id = $1")
        .bind(cl)
        .execute(&app.db.owner)
        .await
        .unwrap();
    let priya_u = create_user(
        &app,
        &org,
        NewUser::employee("priya@acme.test").role("approver"),
    )
    .await;
    let vik_u = create_user(
        &app,
        &org,
        NewUser::employee("vik@acme.test").role("approver"),
    )
    .await;
    let eng = create_group(&app, &org, "Eng", &[&priya_u]).await;
    let sales = create_group(&app, &org, "Sales", &[&vik_u]).await;
    let asha = create_user(&app, &org, NewUser::employee("asha@acme.test").group(eng)).await;
    let ravi = create_user(&app, &org, NewUser::employee("ravi@acme.test").group(eng)).await;
    let sam = create_user(&app, &org, NewUser::employee("sam@acme.test").group(sales)).await;
    set_department(&app, &asha, "Platform").await;
    set_department(&app, &sam, "Platform").await;
    for u in [&asha, &ravi, &sam] {
        add_ledger_row(&app, u, cl, 2026, dec!(12), "allotment").await;
    }
    let priya = app.login_as(&priya_u).await;
    let vik = app.login_as(&vik_u).await;
    World {
        app,
        org,
        admin,
        priya,
        vik,
        asha,
        ravi,
        sam,
        sales,
        cl,
    }
}

async fn inbox(w: &World, client: &reqwest::Client) -> serde_json::Value {
    let (status, out) = get_json(
        client,
        w.app.url("/api/v1/approvals?status=pending&kind=leave"),
    )
    .await;
    assert_eq!(status, 200, "{out}");
    out
}

fn requesters(items: &serde_json::Value) -> Vec<String> {
    items
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["requester"]["full_name"].as_str().unwrap().to_string())
        .collect()
}

#[tokio::test]
async fn approvers_see_their_groups_requests_oldest_first() {
    let w = world().await;
    let asha = w.app.login_as(&w.asha).await;
    let ravi = w.app.login_as(&w.ravi).await;
    let sam = w.app.login_as(&w.sam).await;
    let mut body = leave_body(w.cl, "2026-11-30", "2026-12-02");
    body["reason"] = json!("Family function");
    submit(&w.app, &asha, &body).await;
    w.app.clock.advance(Duration::hours(1));
    submit(&w.app, &ravi, &leave_body(w.cl, "2026-12-14", "2026-12-14")).await;
    submit(&w.app, &sam, &leave_body(w.cl, "2026-12-15", "2026-12-15")).await;

    let items = inbox(&w, &w.priya).await;
    assert_eq!(requesters(&items), ["asha", "ravi"]);
    let first = &items[0];
    assert_eq!(first["kind"], "leave");
    assert_eq!(first["leave_type"]["code"], "CL");
    assert_eq!(first["reason"], "Family function");
    assert_eq!(first["days"], 3.0);
    assert_eq!(first["version"], 1);
    // The requester's available balance for that leave year (12 − 3 pending).
    assert_eq!(first["balance"]["available"], 9.0);

    assert_eq!(requesters(&inbox(&w, &w.vik).await), ["sam"]);
    assert_eq!(
        requesters(&inbox(&w, &w.admin).await),
        ["asha", "ravi", "sam"]
    );

    // Employees have no inbox.
    let (status, _) = get_json(&asha, w.app.url("/api/v1/approvals")).await;
    assert_eq!(status, 403);
}

#[tokio::test]
async fn requests_without_a_group_go_to_every_approver() {
    let w = world().await;
    let meena = create_user(&w.app, &w.org, NewUser::employee("meena@acme.test")).await;
    add_ledger_row(&w.app, &meena, w.cl, 2026, dec!(12), "allotment").await;
    submit(
        &w.app,
        &w.app.login_as(&meena).await,
        &leave_body(w.cl, "2026-12-01", "2026-12-01"),
    )
    .await;
    assert_eq!(requesters(&inbox(&w, &w.priya).await), ["meena"]);
    assert_eq!(requesters(&inbox(&w, &w.vik).await), ["meena"]);
}

#[tokio::test]
async fn team_away_shows_details_only_to_eligible_viewers() {
    let w = world().await;
    // Ravi (Eng) and Sam (Sales, same department as Asha) are away on 1 Dec, approved.
    for (user, approver) in [(&w.ravi, &w.priya), (&w.sam, &w.vik)] {
        let mut body = leave_body(w.cl, "2026-12-01", "2026-12-01");
        body["reason"] = json!(format!("{} private reason", user.email));
        let id = submit(&w.app, &w.app.login_as(user).await, &body).await;
        let (status, out) = decide(&w.app, approver, &id, approve()).await;
        assert_eq!(status, 200, "{out}");
    }
    submit(
        &w.app,
        &w.app.login_as(&w.asha).await,
        &leave_body(w.cl, "2026-11-30", "2026-12-02"),
    )
    .await;

    let items = inbox(&w, &w.priya).await;
    let away = items[0]["team_away"].as_array().unwrap();
    let by_name = |n: &str| away.iter().find(|a| a["full_name"] == n).unwrap().clone();
    assert_eq!(away.len(), 2, "{away:?}");
    let ravi = by_name("ravi");
    assert_eq!(ravi["from"], "2026-12-01");
    assert_eq!(ravi["to"], "2026-12-01");
    assert_eq!(ravi["leave_type"]["code"], "CL");
    assert_eq!(ravi["reason"], "ravi@acme.test private reason");
    // Priya does not approve Sam's group: no type or reason keys at all.
    let sam = by_name("sam");
    assert!(sam.get("leave_type").is_none(), "{sam}");
    assert!(sam.get("reason").is_none(), "{sam}");

    // The admin sees both in full.
    let items = inbox(&w, &w.admin).await;
    let away = items[0]["team_away"].as_array().unwrap();
    assert!(away.iter().all(|a| a.get("reason").is_some()));
}

#[tokio::test]
async fn a_group_move_keeps_the_request_with_its_original_approvers() {
    let w = world().await;
    let id = submit(
        &w.app,
        &w.app.login_as(&w.asha).await,
        &leave_body(w.cl, "2026-12-01", "2026-12-01"),
    )
    .await;
    set_group(&w.app, &w.asha, Some(w.sales)).await;

    assert_eq!(requesters(&inbox(&w, &w.priya).await), ["asha"]);
    assert!(requesters(&inbox(&w, &w.vik).await).is_empty());
    let (status, out) = decide(&w.app, &w.vik, &id, approve()).await;
    assert_eq!(status, 403);
    assert_eq!(out["error"]["code"], "NOT_ELIGIBLE_APPROVER");
    let (status, _) = decide(&w.app, &w.priya, &id, approve()).await;
    assert_eq!(status, 200);
    // Decided requests leave every inbox.
    assert!(requesters(&inbox(&w, &w.priya).await).is_empty());
    assert!(requesters(&inbox(&w, &w.admin).await).is_empty());
}
