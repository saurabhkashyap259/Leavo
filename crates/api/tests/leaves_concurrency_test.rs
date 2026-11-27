//! T094 — simultaneous submissions can neither overspend nor double-book (FR-019).
mod common;

use common::factories::*;
use common::*;
use rust_decimal_macros::dec;

async fn statuses(
    app: &TestApp,
    client: &reqwest::Client,
    bodies: Vec<serde_json::Value>,
) -> Vec<(u16, serde_json::Value)> {
    let keys: Vec<String> = bodies.iter().map(|_| key()).collect();
    let futures = bodies
        .iter()
        .zip(&keys)
        .map(|(b, k)| post_leave(app, client, b, k));
    futures::future::join_all(futures).await
}

#[tokio::test]
async fn ten_requests_for_the_last_day_book_exactly_one() {
    let app = TestApp::spawn().await;
    let (org, _admin, _) = org_with_admin(&app, "acme.test").await;
    let cl = create_leave_type(&app, &org, NewLeaveType::all("CL", dec!(12))).await;
    let asha = create_user(&app, &org, NewUser::employee("asha@acme.test")).await;
    add_ledger_row(&app, &asha, cl, 2026, dec!(1), "allotment").await;
    let client = app.login_as(&asha).await;

    // Ten different working days in December, each needing the one day left.
    let days = [1, 2, 3, 4, 7, 8, 9, 10, 11, 14];
    let bodies = days
        .iter()
        .map(|d| {
            let date = format!("2026-12-{d:02}");
            leave_body(cl, &date, &date)
        })
        .collect();
    let results = statuses(&app, &client, bodies).await;
    let created = results.iter().filter(|(s, _)| *s == 201).count();
    assert_eq!(created, 1, "{results:?}");
    for (s, body) in results.iter().filter(|(s, _)| *s != 201) {
        assert_eq!(*s, 422);
        assert_eq!(body["error"]["code"], "INSUFFICIENT_BALANCE");
    }
    assert_eq!(
        balance_card(&app, &client, "CL", None).await["available"],
        0.0
    );
}

#[tokio::test]
async fn two_overlapping_requests_book_one() {
    let app = TestApp::spawn().await;
    let (org, _admin, _) = org_with_admin(&app, "acme.test").await;
    let cl = create_leave_type(&app, &org, NewLeaveType::all("CL", dec!(12))).await;
    let sl = create_leave_type(&app, &org, NewLeaveType::all("SL", dec!(6))).await;
    let asha = create_user(&app, &org, NewUser::employee("asha@acme.test")).await;
    add_ledger_row(&app, &asha, cl, 2026, dec!(12), "allotment").await;
    add_ledger_row(&app, &asha, sl, 2026, dec!(6), "allotment").await;
    let client = app.login_as(&asha).await;

    for _ in 0..3 {
        // Same type, same dates.
        sqlx::query("DELETE FROM leave_requests")
            .execute(&app.db.owner)
            .await
            .unwrap();
        let results = statuses(
            &app,
            &client,
            vec![
                leave_body(cl, "2026-12-01", "2026-12-02"),
                leave_body(cl, "2026-12-02", "2026-12-03"),
            ],
        )
        .await;
        assert_eq!(
            results.iter().filter(|(s, _)| *s == 201).count(),
            1,
            "{results:?}"
        );

        // Different types: a full day against a half day on the same date.
        sqlx::query("DELETE FROM leave_requests")
            .execute(&app.db.owner)
            .await
            .unwrap();
        let mut half = leave_body(sl, "2026-12-01", "2026-12-01");
        half["start_half"] = serde_json::json!("first");
        let results = statuses(
            &app,
            &client,
            vec![leave_body(cl, "2026-12-01", "2026-12-01"), half],
        )
        .await;
        assert_eq!(
            results.iter().filter(|(s, _)| *s == 201).count(),
            1,
            "{results:?}"
        );
        let loser = results.iter().find(|(s, _)| *s != 201).unwrap();
        assert_eq!(loser.1["error"]["code"], "OVERLAP");
    }
}
