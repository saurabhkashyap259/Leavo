//! T194 — importing from a generic CSV or a Keka export (FR-043): dry run writes nothing; any
//! error blocks the apply; the apply creates invited people, sets balances with `import`
//! entries (a compensating entry on re-import), and turns history into approved leave that
//! blocks overlaps without being charged again.
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
    asha_user: User,
    asha: reqwest::Client,
    cl: Uuid,
}

async fn world() -> World {
    let app = TestApp::spawn().await;
    let (org, _, admin) = org_with_admin(&app, "acme.test").await;
    let cl = create_leave_type(&app, &org, NewLeaveType::all("CL", dec!(12))).await;
    create_leave_type(&app, &org, NewLeaveType::all("SL", dec!(6))).await;
    sqlx::query("UPDATE leave_types SET name = 'Casual Leave' WHERE id = $1")
        .bind(cl)
        .execute(&app.db.owner)
        .await
        .unwrap();
    create_user(
        &app,
        &org,
        NewUser::employee("priya@acme.test").role("approver"),
    )
    .await;
    let asha_user = create_user(&app, &org, NewUser::employee("asha@acme.test")).await;
    add_ledger_row(&app, &asha_user, cl, 2026, dec!(12), "allotment").await;
    World {
        asha: app.login_as(&asha_user).await,
        asha_user,
        admin,
        cl,
        app,
    }
}

const BALANCES: &str = "email,full_name,leave_type_code,leave_year,available_days,approver_group\n\
asha@acme.test,,CL,2026,8,\n\
new@acme.test,New Person,CL,2026,12,Ops\n\
new@acme.test,,SL,2026,3.5,\n";

const HISTORY: &str = "email,leave_type_code,from_date,to_date,days,reason,approved_by_email\n\
asha@acme.test,CL,2026-11-30,2026-12-01,2,Trip,priya@acme.test\n\
new@acme.test,SL,2026-10-05,2026-10-05,1,,\n";

async fn import(
    w: &World,
    client: &reqwest::Client,
    kind: &str,
    parts: &[(&str, &str)],
    dry_run: bool,
) -> (u16, Value) {
    let mut form = reqwest::multipart::Form::new();
    for (name, body) in parts {
        let part = if name.ends_with("map") || *name == "leave_year" {
            reqwest::multipart::Part::text(body.to_string())
        } else {
            reqwest::multipart::Part::bytes(body.as_bytes().to_vec())
                .file_name(format!("{name}.csv"))
        };
        form = form.part(name.to_string(), part);
    }
    let res = client
        .post(
            w.app
                .url(&format!("/api/v1/admin/import/{kind}?dry_run={dry_run}")),
        )
        .multipart(form)
        .send()
        .await
        .unwrap();
    let status = res.status().as_u16();
    (status, res.json().await.unwrap_or(Value::Null))
}

async fn counts(w: &World) -> (i64, i64, i64) {
    let q = |sql: &'static str| {
        let pool = w.app.db.owner.clone();
        async move {
            sqlx::query_scalar::<_, i64>(sql)
                .fetch_one(&pool)
                .await
                .unwrap()
        }
    };
    (
        q("SELECT count(*) FROM users").await,
        q("SELECT count(*) FROM balance_ledger").await,
        q("SELECT count(*) FROM leave_requests").await,
    )
}

async fn balance(w: &World, email: &str, code: &str) -> Decimal {
    sqlx::query_scalar(
        "SELECT COALESCE(SUM(l.delta), 0) FROM balance_ledger l
         JOIN users u ON u.id = l.user_id JOIN leave_types t ON t.id = l.leave_type_id
         WHERE u.email = $1 AND t.code = $2 AND l.leave_year = 2026",
    )
    .bind(email)
    .bind(code)
    .fetch_one(&w.app.db.owner)
    .await
    .unwrap()
}

#[tokio::test]
async fn a_dry_run_reports_and_writes_nothing() {
    let w = world().await;
    let before = counts(&w).await;
    let (status, out) = import(
        &w,
        &w.admin,
        "csv",
        &[("balances", BALANCES), ("history", HISTORY)],
        true,
    )
    .await;
    assert_eq!(status, 200, "{out}");
    assert_eq!(out["dry_run"], true);
    assert_eq!(
        out["users_to_create"],
        json!([{ "email": "new@acme.test", "full_name": "New Person", "approver_group": "Ops" }])
    );
    assert_eq!(out["groups_to_create"], json!(["Ops"]));
    assert_eq!(out["balances_to_set"].as_array().unwrap().len(), 3);
    assert_eq!(out["balances_to_set"][0]["available_days"], 8.0);
    assert_eq!(out["history_rows"].as_array().unwrap().len(), 2);
    assert_eq!(out["errors"], json!([]));
    let warnings: Vec<&str> = out["warnings"]
        .as_array()
        .unwrap()
        .iter()
        .map(|w| w["code"].as_str().unwrap())
        .collect();
    assert_eq!(warnings, ["USER_EXISTS", "GROUP_WILL_BE_CREATED"]);
    assert_eq!(counts(&w).await, before);
}

#[tokio::test]
async fn any_error_blocks_the_whole_apply() {
    let w = world().await;
    let before = counts(&w).await;
    let bad = format!("{BALANCES}x@gmail.com,X,CL,2026,1,\n");
    let (status, out) = import(&w, &w.admin, "csv", &[("balances", &bad)], true).await;
    assert_eq!(status, 200);
    assert_eq!(out["errors"][0]["code"], "UNKNOWN_DOMAIN");
    assert_eq!(out["errors"][0]["file"], "balances");
    assert_eq!(out["errors"][0]["row"], 5);
    let (status, out) = import(&w, &w.admin, "csv", &[("balances", &bad)], false).await;
    assert_eq!(status, 422, "{out}");
    assert_eq!(out["error"]["code"], "IMPORT_HAS_ERRORS");
    assert_eq!(counts(&w).await, before, "nothing written");
}

#[tokio::test]
async fn applying_sets_balances_creates_people_and_records_history() {
    let w = world().await;
    let (status, out) = import(
        &w,
        &w.admin,
        "csv",
        &[("balances", BALANCES), ("history", HISTORY)],
        false,
    )
    .await;
    assert_eq!(status, 200, "{out}");
    assert_eq!(out["dry_run"], false);

    // Balances equal the file, as `import` entries.
    assert_eq!(balance(&w, "asha@acme.test", "CL").await, dec!(8));
    assert_eq!(balance(&w, "new@acme.test", "CL").await, dec!(12));
    assert_eq!(balance(&w, "new@acme.test", "SL").await, dec!(3.5));
    let reasons: Vec<String> = ledger_rows(&w.app, w.asha_user.id, w.cl)
        .await
        .into_iter()
        .map(|r| r.0)
        .collect();
    assert_eq!(reasons, ["allotment", "import"]);
    let (status, new) = get_json(&w.admin, w.app.url("/api/v1/admin/users?q=new@")).await;
    assert_eq!(status, 200);
    assert_eq!(new["items"][0]["status"], "invited");
    assert_eq!(new["items"][0]["approver_group"]["name"], "Ops");
    let new_id: Uuid = new["items"][0]["id"].as_str().unwrap().parse().unwrap();
    let new_reasons: Vec<String> = ledger_rows(&w.app, new_id, w.cl)
        .await
        .into_iter()
        .map(|r| r.0)
        .collect();
    assert_eq!(
        new_reasons,
        ["import"],
        "no allotment on top of the imported balance"
    );

    // History: approved leave, in the employee's history, not charged again, blocking overlaps.
    let (_, history) = get_json(&w.asha, w.app.url("/api/v1/me/leaves")).await;
    let trip = &history["items"][0];
    assert_eq!(trip["status"], "approved");
    assert_eq!(trip["from"], "2026-11-30");
    assert_eq!(trip["days"], 2.0);
    assert_eq!(trip["reason"], "Trip");
    let taken: i64 =
        sqlx::query_scalar("SELECT count(*) FROM balance_ledger WHERE reason = 'leave_taken'")
            .fetch_one(&w.app.db.owner)
            .await
            .unwrap();
    assert_eq!(taken, 0);
    let card = balance_card(&w.app, &w.asha, "CL", None).await;
    assert_eq!(card["available"], 8.0);
    let (status, out) = post_leave(
        &w.app,
        &w.asha,
        &leave_body(w.cl, "2026-12-01", "2026-12-01"),
        &key(),
    )
    .await;
    assert_eq!(status, 422);
    assert_eq!(out["error"]["code"], "OVERLAP");
    let audit: i64 =
        sqlx::query_scalar("SELECT count(*) FROM audit_log WHERE action = 'import.apply'")
            .fetch_one(&w.app.db.owner)
            .await
            .unwrap();
    assert_eq!(audit, 1);

    // Importing again replaces the balance with a compensating entry; history is not repeated.
    let again = "email,leave_type_code,leave_year,available_days\nasha@acme.test,CL,2026,10\n";
    let (_, dry) = import(&w, &w.admin, "csv", &[("balances", again)], true).await;
    let warnings: Vec<&str> = dry["warnings"]
        .as_array()
        .unwrap()
        .iter()
        .map(|w| w["code"].as_str().unwrap())
        .collect();
    assert_eq!(warnings, ["USER_EXISTS", "BALANCE_REPLACES_EARLIER_IMPORT"]);
    let (status, _) = import(&w, &w.admin, "csv", &[("balances", again)], false).await;
    assert_eq!(status, 200);
    assert_eq!(balance(&w, "asha@acme.test", "CL").await, dec!(10));
    let (_, dry) = import(
        &w,
        &w.admin,
        "csv",
        &[
            (
                "balances",
                "email,leave_type_code,leave_year,available_days\n",
            ),
            ("history", HISTORY),
        ],
        true,
    )
    .await;
    let codes: Vec<&str> = dry["errors"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["code"].as_str().unwrap())
        .collect();
    assert_eq!(
        codes,
        ["OVERLAP", "OVERLAP"],
        "the same history again overlaps what was imported"
    );
}

const KEKA_BALANCES: &str = "Employee Number,Employee Name,Email,Leave Type,Available Balance\n\
E-1,Asha Rao,asha@acme.test,Casual Leave,7\n\
E-9,Kiran,kiran@acme.test,Privilege Leave,4\n";

#[tokio::test]
async fn keka_exports_are_mapped_by_leave_type_name() {
    let w = world().await;
    let (status, out) = import(&w, &w.admin, "keka", &[("balances", KEKA_BALANCES)], true).await;
    assert_eq!(status, 200, "{out}");
    // Names matching a Leavo type are suggested; others must be mapped.
    assert_eq!(
        out["keka_leave_types"],
        json!([{ "name": "Casual Leave", "code": "CL" }, { "name": "Privilege Leave", "code": null }])
    );
    assert_eq!(out["errors"][0]["code"], "UNKNOWN_LEAVE_TYPE");

    let map = json!({ "Casual Leave": "CL", "Privilege Leave": "SL" }).to_string();
    let parts = [
        ("balances", KEKA_BALANCES),
        ("leave_type_map", map.as_str()),
        ("leave_year", "2026"),
    ];
    let (_, out) = import(&w, &w.admin, "keka", &parts, true).await;
    assert_eq!(out["errors"], json!([]), "{out}");
    let (status, out) = import(&w, &w.admin, "keka", &parts, false).await;
    assert_eq!(status, 200, "{out}");
    assert_eq!(balance(&w, "asha@acme.test", "CL").await, dec!(7));
    assert_eq!(balance(&w, "kiran@acme.test", "SL").await, dec!(4));

    let (status, _) = import(&w, &w.asha, "keka", &parts, true).await;
    assert_eq!(status, 403);
    let (status, out) = import(&w, &w.admin, "csv", &[], true).await;
    assert_eq!(status, 422, "{out}");
    assert_eq!(out["error"]["details"]["field"], "balances");
}
