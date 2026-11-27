//! T178 — leave taken in a leave year, by type, employee or month, with CSV (FR-044).
//! Counted from booked days of approved leave (and the days that stayed taken when started
//! leave was cancelled); pending, rejected and withdrawn requests do not count.
mod common;

use common::factories::*;
use common::*;
use rust_decimal_macros::dec;
use serde_json::{json, Value};

#[tokio::test]
async fn reports_aggregate_days_taken() {
    let app = TestApp::spawn().await;
    let (org, _, admin) = org_with_admin(&app, "acme.test").await;
    let cl = create_leave_type(&app, &org, NewLeaveType::all("CL", dec!(12))).await;
    let sl = create_leave_type(&app, &org, NewLeaveType::all("SL", dec!(6))).await;
    let asha = create_user(&app, &org, NewUser::employee("asha@acme.test")).await;
    let ravi = create_user(&app, &org, NewUser::employee("ravi@acme.test")).await;
    set_full_name(&app, &asha, "Asha Rao").await;
    set_full_name(&app, &ravi, "Ravi K").await;
    for u in [&asha, &ravi] {
        add_ledger_row(&app, u, cl, 2026, dec!(12), "allotment").await;
        add_ledger_row(&app, u, sl, 2026, dec!(6), "allotment").await;
        add_ledger_row(&app, u, cl, 2027, dec!(12), "allotment").await;
    }
    let asha_c = app.login_as(&asha).await;
    let ravi_c = app.login_as(&ravi).await;
    let approved = |client: reqwest::Client, body: Value| {
        let (app, admin) = (&app, admin.clone());
        async move {
            let id = submit(app, &client, &body).await;
            let (s, out) = decide(app, &admin, &id, approve()).await;
            assert_eq!(s, 200, "{out}");
        }
    };
    // Asha: CL 30 Nov – 2 Dec (1 day in Nov, 2 in Dec) and SL 7 Dec; Ravi: CL 14 Dec, half day.
    approved(asha_c.clone(), leave_body(cl, "2026-11-30", "2026-12-02")).await;
    approved(asha_c.clone(), leave_body(sl, "2026-12-07", "2026-12-07")).await;
    let mut half = leave_body(cl, "2026-12-14", "2026-12-14");
    half["start_half"] = json!("first");
    approved(ravi_c.clone(), half).await;
    // Not counted: pending, and leave in the next leave year.
    submit(&app, &ravi_c, &leave_body(cl, "2026-12-21", "2026-12-21")).await;
    approved(asha_c.clone(), leave_body(cl, "2027-01-04", "2027-01-04")).await;

    let report = |q: &str| {
        let (client, url) = (
            admin.clone(),
            app.url(&format!("/api/v1/admin/reports/leaves?{q}")),
        );
        async move {
            let (status, out) = get_json(&client, url).await;
            assert_eq!(status, 200, "{out}");
            out.as_array()
                .unwrap()
                .iter()
                .map(|r| {
                    (
                        r["group"].as_str().unwrap().to_string(),
                        r["leave_type"]["code"].as_str().unwrap().to_string(),
                        r["days"].as_f64().unwrap(),
                    )
                })
                .collect::<Vec<_>>()
        }
    };
    let g = |a: &str, b: &str, c: f64| (a.to_string(), b.to_string(), c);
    // Grouped by type, the group is the type's name (the factory names types by code).
    assert_eq!(
        report("group_by=type&year=2026").await,
        [g("CL", "CL", 3.5), g("SL", "SL", 1.0)]
    );
    assert_eq!(
        report("group_by=user&year=2026").await,
        [
            g("Asha Rao", "CL", 3.0),
            g("Asha Rao", "SL", 1.0),
            g("Ravi K", "CL", 0.5)
        ]
    );
    assert_eq!(
        report("group_by=month&year=2026").await,
        [
            g("2026-11", "CL", 1.0),
            g("2026-12", "CL", 2.5),
            g("2026-12", "SL", 1.0)
        ]
    );
    assert_eq!(
        report("group_by=type&year=2027").await,
        [g("CL", "CL", 1.0)]
    );

    let res = admin
        .get(app.url("/api/v1/admin/reports/leaves.csv?group_by=user&year=2026"))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 200);
    assert!(res.headers()["content-type"]
        .to_str()
        .unwrap()
        .starts_with("text/csv"));
    assert_eq!(
        res.text().await.unwrap(),
        "group,leave_type,days\r\nAsha Rao,CL,3\r\nAsha Rao,SL,1\r\nRavi K,CL,0.5\r\n"
    );

    for q in ["group_by=team&year=2026", "group_by=type&year=99"] {
        let (status, _) = get_json(
            &admin,
            app.url(&format!("/api/v1/admin/reports/leaves?{q}")),
        )
        .await;
        assert_eq!(status, 422, "{q}");
    }
    let (status, _) = get_json(
        &asha_c,
        app.url("/api/v1/admin/reports/leaves?group_by=type"),
    )
    .await;
    assert_eq!(status, 403);
}
