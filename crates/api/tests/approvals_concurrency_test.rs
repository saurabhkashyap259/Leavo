//! T113 — two approvers deciding at the same moment: exactly one decision is recorded (FR-023).
mod common;

use common::factories::*;
use common::*;
use rust_decimal_macros::dec;
use serde_json::json;

#[tokio::test]
async fn simultaneous_decisions_record_one() {
    let app = TestApp::spawn().await;
    let (org, _admin_user, admin) = org_with_admin(&app, "acme.test").await;
    let cl = create_leave_type(&app, &org, NewLeaveType::all("CL", dec!(12))).await;
    let priya = create_user(
        &app,
        &org,
        NewUser::employee("priya@acme.test").role("approver"),
    )
    .await;
    let asha = create_user(&app, &org, NewUser::employee("asha@acme.test")).await;
    add_ledger_row(&app, &asha, cl, 2026, dec!(12), "allotment").await;
    let asha_client = app.login_as(&asha).await;
    let priya = app.login_as(&priya).await;

    // Five weekdays in December.
    for d in [1, 2, 3, 4, 7] {
        let day = format!("2026-12-{d:02}");
        let id = submit(&app, &asha_client, &leave_body(cl, &day, &day)).await;
        let reject = json!({ "decision": "reject", "version": 1, "comment": "No" });
        let (a, b) = tokio::join!(
            decide(&app, &priya, &id, approve()),
            decide(&app, &admin, &id, reject)
        );
        let mut statuses = [a.0, b.0];
        statuses.sort();
        assert_eq!(statuses, [200, 409], "{a:?} {b:?}");
        let loser = if a.0 == 409 { &a.1 } else { &b.1 };
        assert_eq!(loser["error"]["code"], "ALREADY_DECIDED");
    }
    // Approved rounds wrote exactly one deduction each.
    let taken: Vec<_> = ledger_rows(&app, asha.id, cl)
        .await
        .into_iter()
        .filter(|r| r.0 == "leave_taken")
        .collect();
    let (approved,): (i64,) =
        sqlx::query_as("SELECT count(*) FROM leave_requests WHERE status = 'approved'")
            .fetch_one(&app.db.owner)
            .await
            .unwrap();
    assert_eq!(taken.len() as i64, approved);
}
