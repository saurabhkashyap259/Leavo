//! T220 — `leavo-demo-seed` fills an empty instance, refuses to overwrite one, and `--reset`
//! rebuilds it from scratch (the nightly demo reset).
use leavo_db::testkit::TestDb;
use std::process::{Command, Output};

async fn seed(url: &str, reset: bool) -> Output {
    seed_with(url, if reset { &["--reset"] } else { &[] }).await
}

async fn seed_with(url: &str, args: &[&str]) -> Output {
    let url = url.to_string();
    let args: Vec<String> = args.iter().map(|a| a.to_string()).collect();
    tokio::task::spawn_blocking(move || {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_leavo-demo-seed"));
        cmd.env_clear()
            .env("DATABASE_URL", url)
            .env("DEMO_DOMAIN", "demo.test")
            .args(args);
        cmd.output().unwrap()
    })
    .await
    .unwrap()
}

async fn count(db: &TestDb, sql: &str) -> i64 {
    sqlx::query_scalar(sql).fetch_one(&db.owner).await.unwrap()
}

#[tokio::test]
async fn seeds_once_and_resets_on_request() {
    let db = TestDb::new().await;
    let out = seed(&db.url, false).await;
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(String::from_utf8_lossy(&out.stdout).contains("admin@demo.test"));
    assert_eq!(count(&db, "SELECT count(*) FROM organizations").await, 1);
    assert_eq!(count(&db, "SELECT count(*) FROM users").await, 11);
    assert_eq!(count(&db, "SELECT count(*) FROM approver_groups").await, 3);
    assert!(count(&db, "SELECT count(*) FROM holidays").await > 10);
    assert!(
        count(
            &db,
            "SELECT count(*) FROM leave_requests WHERE status = 'approved'"
        )
        .await
            > 0
    );
    assert!(
        count(
            &db,
            "SELECT count(*) FROM leave_requests WHERE status = 'pending'"
        )
        .await
            > 0
    );
    assert_eq!(
        count(
            &db,
            "SELECT count(*) FROM instance_settings WHERE setup_completed_at IS NOT NULL"
        )
        .await,
        1
    );
    // Balances equal their entries (approved leave was charged).
    assert!(
        count(
            &db,
            "SELECT count(*) FROM balance_ledger WHERE reason = 'leave_taken'"
        )
        .await
            > 0
    );

    let again = seed(&db.url, false).await;
    assert!(!again.status.success());
    assert!(String::from_utf8_lossy(&again.stderr).contains("--reset"));

    let reset = seed(&db.url, true).await;
    assert!(
        reset.status.success(),
        "{}",
        String::from_utf8_lossy(&reset.stderr)
    );
    assert_eq!(count(&db, "SELECT count(*) FROM organizations").await, 1);
    assert_eq!(count(&db, "SELECT count(*) FROM users").await, 11);
}

/// T225 — the performance check seeds a large organization: `--employees N` adds N generated
/// employees (spread over the approver groups, each with allotments and some leave).
#[tokio::test]
async fn seeds_extra_employees_for_load_tests() {
    let db = TestDb::new().await;
    let out = seed_with(&db.url, &["--employees", "40"]).await;
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(count(&db, "SELECT count(*) FROM users").await, 51);
    assert_eq!(
        count(
            &db,
            "SELECT count(*) FROM users WHERE role = 'employee' AND approver_group_id IS NULL"
        )
        .await,
        0
    );
    // Every generated employee has allotments.
    assert_eq!(
        count(
            &db,
            "SELECT count(*) FROM users u WHERE NOT EXISTS
               (SELECT 1 FROM balance_ledger b WHERE b.user_id = u.id AND b.reason = 'allotment')"
        )
        .await,
        0
    );
    assert!(count(&db, "SELECT count(*) FROM leave_requests").await > 30);

    let bad = seed_with(&db.url, &["--reset", "--employees", "lots"]).await;
    assert!(!bad.status.success());
    assert!(String::from_utf8_lossy(&bad.stderr).contains("--employees"));
}
