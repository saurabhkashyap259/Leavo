//! `GET /holidays?year=` (FR-033). US5 needs the read side for the dashboard's upcoming
//! holidays; US8 (T158) adds admin CRUD and imports (FR-033, FR-034).
mod common;

use chrono::NaiveDate;
use common::factories::*;
use common::*;

fn d(s: &str) -> NaiveDate {
    s.parse().unwrap()
}

#[tokio::test]
async fn lists_the_current_and_next_years_holidays_by_date() {
    let app = TestApp::spawn().await;
    let (org, _, _) = org_with_admin(&app, "acme.test").await;
    let other = create_org(&app, "Other", "other.test").await;
    add_holiday(&app, &org, d("2026-12-25"), "Christmas", "mandatory").await;
    add_holiday(&app, &org, d("2026-11-08"), "Diwali", "mandatory").await;
    add_holiday(&app, &org, d("2026-12-31"), "New Year's Eve", "optional").await;
    add_holiday(&app, &org, d("2027-01-26"), "Republic Day", "mandatory").await;
    add_holiday(&app, &other, d("2026-12-01"), "Not ours", "mandatory").await;
    let emp = create_user(&app, &org, NewUser::employee("asha@acme.test")).await;
    let asha = app.login_as(&emp).await;

    // Defaults to the current leave year.
    let (status, out) = get_json(&asha, app.url("/api/v1/holidays")).await;
    assert_eq!(status, 200, "{out}");
    let items = out.as_array().unwrap();
    let dates: Vec<&str> = items.iter().map(|h| h["date"].as_str().unwrap()).collect();
    assert_eq!(dates, ["2026-11-08", "2026-12-25", "2026-12-31"]);
    assert_eq!(items[2]["name"], "New Year's Eve");
    assert_eq!(items[2]["kind"], "optional");
    assert!(items[0]["id"].is_string());

    let (status, out) = get_json(&asha, app.url("/api/v1/holidays?year=2027")).await;
    assert_eq!(status, 200);
    assert_eq!(out[0]["name"], "Republic Day");

    // Employees see the current and next year only.
    for y in ["2025", "2028"] {
        let (status, out) = get_json(&asha, app.url(&format!("/api/v1/holidays?year={y}"))).await;
        assert_eq!(status, 422, "{y}: {out}");
        assert_eq!(out["error"]["details"]["field"], "year");
    }
    let (status, _) = get_json(&app.anon(), app.url("/api/v1/holidays")).await;
    assert_eq!(status, 401);
}

struct Admin {
    app: TestApp,
    org: Org,
    admin: reqwest::Client,
    asha: reqwest::Client,
}

async fn admin_world() -> Admin {
    let app = TestApp::spawn().await;
    let (org, _, admin) = org_with_admin(&app, "acme.test").await;
    let emp = create_user(&app, &org, NewUser::employee("asha@acme.test")).await;
    let asha = app.login_as(&emp).await;
    Admin {
        app,
        org,
        admin,
        asha,
    }
}

async fn holidays(w: &Admin, year: i32) -> Vec<(String, String, String)> {
    let (status, out) = get_json(
        &w.admin,
        w.app.url(&format!("/api/v1/holidays?year={year}")),
    )
    .await;
    assert_eq!(status, 200, "{out}");
    out.as_array()
        .unwrap()
        .iter()
        .map(|h| {
            (
                h["date"].as_str().unwrap().into(),
                h["name"].as_str().unwrap().into(),
                h["kind"].as_str().unwrap().into(),
            )
        })
        .collect()
}

fn h(date: &str, name: &str, kind: &str) -> (String, String, String) {
    (date.into(), name.into(), kind.into())
}

#[tokio::test]
async fn admins_add_edit_and_delete_holidays() {
    let w = admin_world().await;
    let body =
        serde_json::json!({ "date": "2026-12-24", "name": "Christmas Eve", "kind": "optional" });
    let (status, _) = post_json(&w.asha, w.app.url("/api/v1/admin/holidays"), body.clone()).await;
    assert_eq!(status, 403);
    let (status, created) =
        post_json(&w.admin, w.app.url("/api/v1/admin/holidays"), body.clone()).await;
    assert_eq!(status, 201, "{created}");
    assert_eq!(created["kind"], "optional");
    assert_eq!(created["location"], serde_json::Value::Null);
    let id = created["id"].as_str().unwrap().to_string();

    for (bad, field) in [
        (body.clone(), "name"),
        (
            serde_json::json!({ "date": "2026-12-26", "name": " ", "kind": "mandatory" }),
            "name",
        ),
        (
            serde_json::json!({ "date": "2026-12-26", "name": "Boxing Day", "kind": "floating" }),
            "kind",
        ),
    ] {
        let (status, out) = post_json(&w.admin, w.app.url("/api/v1/admin/holidays"), bad).await;
        assert_eq!(status, 422, "{out}");
        assert_eq!(out["error"]["details"]["field"], field, "{out}");
    }

    let (status, out) = patch_json(
        &w.admin,
        w.app.url(&format!("/api/v1/admin/holidays/{id}")),
        serde_json::json!({ "name": "Xmas Eve", "kind": "mandatory", "location": "Bengaluru" }),
    )
    .await;
    assert_eq!(status, 200, "{out}");
    assert_eq!(out["location"], "Bengaluru");
    assert_eq!(
        holidays(&w, 2026).await,
        [h("2026-12-24", "Xmas Eve", "mandatory")]
    );
    let (status, _) = patch_json(
        &w.asha,
        w.app.url(&format!("/api/v1/admin/holidays/{id}")),
        serde_json::json!({ "name": "Mine" }),
    )
    .await;
    assert_eq!(status, 403);

    let (status, _) = delete(&w.admin, w.app.url(&format!("/api/v1/admin/holidays/{id}"))).await;
    assert_eq!(status, 204);
    assert!(holidays(&w, 2026).await.is_empty());
    let (status, _) = delete(&w.admin, w.app.url(&format!("/api/v1/admin/holidays/{id}"))).await;
    assert_eq!(status, 404);

    // Admins may look at any year; employees only the current and next.
    let (status, _) = get_json(&w.admin, w.app.url("/api/v1/holidays?year=2025")).await;
    assert_eq!(status, 200);
}

async fn import_csv(w: &Admin, csv: &str, dry_run: bool) -> (u16, serde_json::Value) {
    let form = reqwest::multipart::Form::new().part(
        "file",
        reqwest::multipart::Part::bytes(csv.as_bytes().to_vec())
            .file_name("holidays.csv")
            .mime_str("text/csv")
            .unwrap(),
    );
    let res = w
        .admin
        .post(
            w.app
                .url(&format!("/api/v1/admin/holidays/import?dry_run={dry_run}")),
        )
        .multipart(form)
        .send()
        .await
        .unwrap();
    let status = res.status().as_u16();
    (status, res.json().await.unwrap_or(serde_json::Value::Null))
}

async fn import_json(
    w: &Admin,
    body: serde_json::Value,
    dry_run: bool,
) -> (u16, serde_json::Value) {
    post_json(
        &w.admin,
        w.app
            .url(&format!("/api/v1/admin/holidays/import?dry_run={dry_run}")),
        body,
    )
    .await
}

const CSV: &str = "date,name,kind,location\n\
2027-01-26,Republic Day,mandatory,\n\
2027-08-15,Independence Day,,\n\
2027-10-02,Gandhi Jayanti,optional,Delhi\n";

#[tokio::test]
async fn csv_import_previews_then_applies_new_rows_only() {
    let w = admin_world().await;
    add_holiday(&w.app, &w.org, d("2027-01-26"), "Republic Day", "mandatory").await;

    let (status, out) = import_csv(&w, CSV, true).await;
    assert_eq!(status, 200, "{out}");
    assert_eq!(out["dry_run"], true);
    assert_eq!(out["added"].as_array().unwrap().len(), 2);
    assert_eq!(
        out["added"][0]["kind"], "mandatory",
        "kind defaults to mandatory"
    );
    assert_eq!(out["added"][1]["location"], "Delhi");
    assert_eq!(
        out["skipped"],
        serde_json::json!([{ "date": "2027-01-26", "name": "Republic Day", "reason": "exists" }])
    );
    assert_eq!(out["errors"], serde_json::json!([]));
    assert_eq!(
        holidays(&w, 2027).await.len(),
        1,
        "a dry run writes nothing"
    );

    let (status, out) = import_csv(&w, CSV, false).await;
    assert_eq!(status, 200, "{out}");
    assert_eq!(out["dry_run"], false);
    assert_eq!(
        holidays(&w, 2027).await,
        [
            h("2027-01-26", "Republic Day", "mandatory"),
            h("2027-08-15", "Independence Day", "mandatory"),
            h("2027-10-02", "Gandhi Jayanti", "optional"),
        ]
    );
}

#[tokio::test]
async fn a_csv_with_errors_is_reported_and_never_applied() {
    let w = admin_world().await;
    let bad = "date,name,kind\n2027-01-26,Republic Day,mandatory\n2027-02-30,Nope,mandatory\n2027-03-01,Holi,sometimes\n2027-01-26,Republic Day,mandatory\n";
    let (status, out) = import_csv(&w, bad, true).await;
    assert_eq!(status, 200, "{out}");
    let codes: Vec<(u64, String)> = out["errors"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| {
            (
                e["row"].as_u64().unwrap(),
                e["code"].as_str().unwrap().into(),
            )
        })
        .collect();
    assert_eq!(
        codes,
        [
            (3, "INVALID_DATE".into()),
            (4, "INVALID_KIND".into()),
            (5, "DUPLICATE_ROW".into())
        ]
    );
    let (status, out) = import_csv(&w, bad, false).await;
    assert_eq!(status, 422, "{out}");
    assert_eq!(out["error"]["code"], "IMPORT_HAS_ERRORS");
    assert!(holidays(&w, 2027).await.is_empty());

    let (_, out) = import_csv(&w, "date,kind\n2027-01-26,mandatory\n", true).await;
    assert_eq!(out["errors"][0]["code"], "MISSING_COLUMN");
}

#[tokio::test]
async fn packs_and_last_years_list_can_be_imported() {
    let w = admin_world().await;
    let (status, out) = import_json(&w, serde_json::json!({ "pack": "IN/2027" }), false).await;
    assert_eq!(status, 200, "{out}");
    assert_eq!(out["added"].as_array().unwrap().len(), 17);
    // Importing again adds nothing.
    let (_, out) = import_json(&w, serde_json::json!({ "pack": "IN/2027" }), false).await;
    assert_eq!(out["added"], serde_json::json!([]));
    assert_eq!(out["skipped"].as_array().unwrap().len(), 17);
    let (status, out) = import_json(&w, serde_json::json!({ "pack": "XX/2027" }), true).await;
    assert_eq!(status, 422);
    assert_eq!(out["error"]["details"]["field"], "pack");

    // Copy 2026 into 2027: same names and kinds, one year on.
    add_holiday(&w.app, &w.org, d("2026-11-08"), "Diwali", "mandatory").await;
    add_holiday(
        &w.app,
        &w.org,
        d("2026-12-31"),
        "New Year's Eve",
        "optional",
    )
    .await;
    let copy = serde_json::json!({ "copy_from_year": 2026, "to_year": 2027 });
    let (status, out) = import_json(&w, copy.clone(), true).await;
    assert_eq!(status, 200, "{out}");
    let added: Vec<(String, String)> = out["added"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| {
            (
                a["date"].as_str().unwrap().into(),
                a["name"].as_str().unwrap().into(),
            )
        })
        .collect();
    assert_eq!(
        added,
        [
            ("2027-11-08".to_string(), "Diwali".to_string()),
            ("2027-12-31".to_string(), "New Year's Eve".to_string())
        ]
    );
    let (status, _) = import_json(&w, copy, false).await;
    assert_eq!(status, 200);
    assert!(holidays(&w, 2027)
        .await
        .contains(&h("2027-12-31", "New Year's Eve", "optional")));

    let (status, _) = post_json(
        &w.asha,
        w.app.url("/api/v1/admin/holidays/import?dry_run=true"),
        serde_json::json!({ "pack": "IN/2027" }),
    )
    .await;
    assert_eq!(status, 403);
}
