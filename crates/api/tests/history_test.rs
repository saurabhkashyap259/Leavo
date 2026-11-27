//! T127 — leave history with filters, pagination and CSV export (FR-028,
//! contracts/csv-formats.md §7).
mod common;

use common::factories::*;
use common::*;
use rust_decimal_macros::dec;
use serde_json::{json, Value};

struct World {
    app: TestApp,
    asha: reqwest::Client,
    sl: uuid::Uuid,
}

/// Asha's requests, newest first:
/// - r3 CL Tue 5 Jan 2027, pending (leave year 2027)
/// - r4 CL Tue 15 – Wed 16 Dec 2026, last day first half, approved
/// - r2 SL Mon 7 Dec 2026, rejected "Busy week"; reason starts with "=" (CSV injection)
/// - r1 CL Tue 1 – Wed 2 Dec 2026, approved "Enjoy!", notified Ravi and client@x.com
///
/// Ravi has a request of his own, never listed in Asha's history.
async fn world() -> World {
    let app = TestApp::spawn().await;
    let (org, _, _admin) = org_with_admin(&app, "acme.test").await;
    let cl = create_leave_type(&app, &org, NewLeaveType::all("CL", dec!(12))).await;
    let sl = create_leave_type(&app, &org, NewLeaveType::all("SL", dec!(6))).await;
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
    set_full_name(&app, &priya_u, "Priya Lead").await;
    let eng = create_group(&app, &org, "Eng", &[&priya_u]).await;
    let asha_u = create_user(&app, &org, NewUser::employee("asha@acme.test").group(eng)).await;
    let ravi_u = create_user(&app, &org, NewUser::employee("ravi@acme.test").group(eng)).await;
    set_full_name(&app, &ravi_u, "Ravi K").await;
    for u in [&asha_u, &ravi_u] {
        add_ledger_row(&app, u, cl, 2026, dec!(12), "allotment").await;
        add_ledger_row(&app, u, sl, 2026, dec!(6), "allotment").await;
    }
    let asha = app.login_as(&asha_u).await;
    let priya = app.login_as(&priya_u).await;

    let mut b1 = leave_body(cl, "2026-12-01", "2026-12-02");
    b1["notify"] = json!([{ "user_id": ravi_u.id }, { "email": "client@x.com" }]);
    let r1 = submit(&app, &asha, &b1).await;
    let (s, out) = decide(
        &app,
        &priya,
        &r1,
        json!({ "decision": "approve", "version": 1, "comment": "Enjoy!" }),
    )
    .await;
    assert_eq!(s, 200, "{out}");

    let mut b2 = leave_body(sl, "2026-12-07", "2026-12-07");
    b2["reason"] = json!("=HYPERLINK(\"x\")");
    let r2 = submit(&app, &asha, &b2).await;
    let (s, _) = decide(
        &app,
        &priya,
        &r2,
        json!({ "decision": "reject", "version": 1, "comment": "Busy week" }),
    )
    .await;
    assert_eq!(s, 200);

    let mut b4 = leave_body(cl, "2026-12-15", "2026-12-16");
    b4["end_half"] = json!("first");
    let r4 = submit(&app, &asha, &b4).await;
    let (s, _) = decide(&app, &priya, &r4, approve()).await;
    assert_eq!(s, 200);

    submit(&app, &asha, &leave_body(cl, "2027-01-05", "2027-01-05")).await;
    submit(
        &app,
        &app.login_as(&ravi_u).await,
        &leave_body(cl, "2026-12-21", "2026-12-21"),
    )
    .await;
    World { app, asha, sl }
}

async fn history(w: &World, query: &str) -> Value {
    let (status, out) = get_json(&w.asha, w.app.url(&format!("/api/v1/me/leaves?{query}"))).await;
    assert_eq!(status, 200, "{out}");
    out
}

fn froms(out: &Value) -> Vec<String> {
    out["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["from"].as_str().unwrap().to_string())
        .collect()
}

#[tokio::test]
async fn history_lists_own_requests_newest_first_with_every_field() {
    let w = world().await;
    let out = history(&w, "").await;
    assert_eq!(
        froms(&out),
        ["2027-01-05", "2026-12-15", "2026-12-07", "2026-12-01"]
    );
    assert!(out["next_cursor"].is_null());

    let r1 = &out["items"][3];
    assert_eq!(r1["kind"], "leave");
    assert_eq!(r1["leave_type"]["code"], "CL");
    assert_eq!(r1["leave_type"]["name"], "Casual Leave");
    assert!(r1["leave_type"]["color"].is_string());
    assert_eq!(r1["to"], "2026-12-02");
    assert_eq!(r1["days"], 2.0);
    assert_eq!(r1["status"], "approved");
    assert!(r1["requested_at"].is_string());
    assert_eq!(r1["decided_by"]["full_name"], "Priya Lead");
    assert!(r1["decided_at"].is_string());
    assert_eq!(r1["reason"], "Family function");
    assert_eq!(r1["decision_comment"], "Enjoy!");
    let mut notify: Vec<String> = r1["notify"]
        .as_array()
        .unwrap()
        .iter()
        .map(|n| n.as_str().unwrap().to_string())
        .collect();
    notify.sort();
    assert_eq!(notify, ["Ravi K", "client@x.com"]);
    assert!(r1["attachment"].is_null());

    let r4 = &out["items"][1];
    assert!(r4["start_half"].is_null());
    assert_eq!(r4["end_half"], "first");
    assert_eq!(r4["days"], 1.5);

    let r3 = &out["items"][0];
    assert_eq!(r3["status"], "pending");
    assert!(r3["decided_by"].is_null());
    assert!(r3["decided_at"].is_null());
}

#[tokio::test]
async fn filters_by_leave_year_type_and_status() {
    let w = world().await;
    assert_eq!(
        froms(&history(&w, "year=2026").await),
        ["2026-12-15", "2026-12-07", "2026-12-01"]
    );
    assert_eq!(froms(&history(&w, "year=2027").await), ["2027-01-05"]);
    assert_eq!(
        froms(&history(&w, &format!("type={}", w.sl)).await),
        ["2026-12-07"]
    );
    assert_eq!(
        froms(&history(&w, "status=approved").await),
        ["2026-12-15", "2026-12-01"]
    );
    assert_eq!(
        froms(&history(&w, "year=2026&status=rejected").await),
        ["2026-12-07"]
    );
    for (q, field) in [
        ("status=maybe", "status"),
        ("type=CL", "type"),
        ("year=99", "year"),
        ("limit=0", "limit"),
        ("cursor=nonsense", "cursor"),
    ] {
        let (status, out) = get_json(&w.asha, w.app.url(&format!("/api/v1/me/leaves?{q}"))).await;
        assert_eq!(status, 422, "{q}: {out}");
        assert_eq!(out["error"]["details"]["field"], field, "{q}");
    }
}

#[tokio::test]
async fn pages_with_an_opaque_cursor() {
    let w = world().await;
    let first = history(&w, "limit=3").await;
    assert_eq!(froms(&first), ["2027-01-05", "2026-12-15", "2026-12-07"]);
    let cursor = first["next_cursor"]
        .as_str()
        .expect("a next page")
        .to_string();
    let second = history(&w, &format!("limit=3&cursor={cursor}")).await;
    assert_eq!(froms(&second), ["2026-12-01"]);
    assert!(second["next_cursor"].is_null());
    // Filters apply across pages.
    let first = history(&w, "limit=1&status=approved").await;
    let cursor = first["next_cursor"].as_str().unwrap().to_string();
    let second = history(&w, &format!("limit=1&status=approved&cursor={cursor}")).await;
    assert_eq!(froms(&second), ["2026-12-01"]);
}

/// Parse a CSV body into header + rows (handles quoted fields).
fn parse_csv(text: &str) -> (Vec<String>, Vec<Vec<String>>) {
    let mut rows = Vec::new();
    let mut row = Vec::new();
    let mut field = String::new();
    let mut quoted = false;
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match (quoted, c) {
            (true, '"') if chars.peek() == Some(&'"') => {
                field.push('"');
                chars.next();
            }
            (true, '"') => quoted = false,
            (true, c) => field.push(c),
            (false, '"') => quoted = true,
            (false, ',') => row.push(std::mem::take(&mut field)),
            (false, '\r') => {}
            (false, '\n') => {
                row.push(std::mem::take(&mut field));
                rows.push(std::mem::take(&mut row));
            }
            (false, c) => field.push(c),
        }
    }
    if !field.is_empty() || !row.is_empty() {
        row.push(field);
        rows.push(row);
    }
    let header = rows.remove(0);
    (header, rows)
}

#[tokio::test]
async fn csv_has_the_documented_columns_and_the_same_fields_as_json() {
    let w = world().await;
    let res = w
        .asha
        .get(w.app.url("/api/v1/me/leaves.csv?year=2026"))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 200);
    assert!(res.headers()["content-type"]
        .to_str()
        .unwrap()
        .starts_with("text/csv"));
    assert!(res.headers()["content-disposition"]
        .to_str()
        .unwrap()
        .contains("attachment"));
    let (header, rows) = parse_csv(&res.text().await.unwrap());
    assert_eq!(
        header.join(","),
        "leave_type,from,to,days,half_day,status,requested_on,decided_by,decided_on,reason,approver_comment,notified"
    );
    let json = history(&w, "year=2026").await;
    let items = json["items"].as_array().unwrap();
    assert_eq!(rows.len(), items.len());
    for (row, item) in rows.iter().zip(items) {
        assert_eq!(row[0], item["leave_type"]["name"].as_str().unwrap());
        assert_eq!(row[1], item["from"].as_str().unwrap());
        assert_eq!(row[2], item["to"].as_str().unwrap());
        assert_eq!(
            row[3].parse::<f64>().unwrap(),
            item["days"].as_f64().unwrap()
        );
        assert_eq!(row[5], item["status"].as_str().unwrap());
        assert_eq!(row[6], item["requested_at"].as_str().unwrap()[..10]);
        assert_eq!(
            row[7],
            item["decided_by"]["full_name"].as_str().unwrap_or("")
        );
        assert_eq!(
            row[8],
            item["decided_at"].as_str().map(|d| &d[..10]).unwrap_or("")
        );
        assert_eq!(row[10], item["decision_comment"].as_str().unwrap_or(""));
    }
    // r4: last day first half; r1: no half day.
    assert_eq!(rows[0][4], "last day: first half");
    assert_eq!(rows[2][4], "");
    // A formula-looking reason is neutralised for spreadsheets.
    assert_eq!(rows[1][9], "'=HYPERLINK(\"x\")");
    assert_eq!(rows[2][9], "Family function");
    let mut notified: Vec<&str> = rows[2][11].split("; ").collect();
    notified.sort();
    assert_eq!(notified, ["Ravi K", "client@x.com"]);
}
