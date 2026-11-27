//! T202 — a failing migration (FR-069): it rolls back completely, the error names its version,
//! earlier migrations stay applied, later ones are not run, and the status reports what is
//! pending (so `/readyz` keeps failing).
use leavo_db::migrate::{run_with, status_of, MigrationFailure};
use leavo_db::testkit::empty_database;
use sqlx::migrate::Migrator;
use std::path::Path;

async fn exists(pool: &sqlx::PgPool, table: &str) -> bool {
    sqlx::query_scalar("SELECT to_regclass($1) IS NOT NULL")
        .bind(table)
        .fetch_one(pool)
        .await
        .unwrap()
}

#[tokio::test]
async fn a_broken_migration_rolls_back_and_names_its_version() {
    let (_, owner) = empty_database().await;
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/broken_migrations");
    let migrator = Migrator::new(dir).await.unwrap();

    let err = run_with(&migrator, &owner).await.unwrap_err();
    assert!(
        matches!(err, MigrationFailure::Failed { version: 2, .. }),
        "{err:?}"
    );
    assert!(err.to_string().starts_with("migration 2 failed:"), "{err}");

    assert!(
        exists(&owner, "fixture_ok").await,
        "earlier migrations stay applied"
    );
    assert!(
        !exists(&owner, "fixture_partial").await,
        "the failed migration rolled back"
    );
    assert!(
        !exists(&owner, "fixture_never").await,
        "later migrations are not run"
    );
    let applied: Vec<i64> =
        sqlx::query_scalar("SELECT version FROM _sqlx_migrations WHERE success ORDER BY version")
            .fetch_all(&owner)
            .await
            .unwrap();
    assert_eq!(applied, [1]);
    assert_eq!(status_of(&migrator, &owner).await.unwrap(), (1, 2));
}

#[tokio::test]
async fn a_database_from_a_newer_release_is_refused() {
    let (_, owner) = empty_database().await;
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/broken_migrations");
    let migrator = Migrator::new(dir).await.unwrap();
    let _ = run_with(&migrator, &owner).await;
    // A row for a migration this binary does not have: the database was upgraded past it.
    sqlx::query(
        "INSERT INTO _sqlx_migrations (version, description, success, checksum, execution_time)
         VALUES (99, 'from a newer release', true, '\\x00', 0)",
    )
    .execute(&owner)
    .await
    .unwrap();
    let ok = Migrator::new(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/broken_migrations"),
    )
    .await
    .unwrap();
    let err = run_with(&ok, &owner).await.unwrap_err();
    assert!(matches!(err, MigrationFailure::Newer(99)), "{err:?}");
    assert!(
        err.to_string().contains("Downgrades are not supported"),
        "{err}"
    );
}
