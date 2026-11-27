//! Forward-only migrations embedded in the binary (FR-050, FR-069).
//!
//! `sqlx` takes a PostgreSQL advisory lock while migrating, so two instances starting together
//! cannot race, and runs each migration inside its own transaction: a failing migration rolls
//! back completely and the ones before it stay applied. The caller must not serve traffic after
//! a failure (`/readyz` keeps reporting the pending migrations).
use sqlx::migrate::{MigrateError, Migrator};
use sqlx::PgPool;

pub static MIGRATOR: Migrator = sqlx::migrate!("../../migrations");

/// Why migrating stopped.
#[derive(Debug, thiserror::Error)]
pub enum MigrationFailure {
    #[error("migration {version} failed: {message}")]
    Failed { version: i64, message: String },
    #[error(
        "the database has migration {0}, which this version of Leavo does not know: it was \
         upgraded by a newer release. Downgrades are not supported; restore the backup taken \
         before the upgrade, or run the newer release"
    )]
    Newer(i64),
    #[error("migrations failed: {0}")]
    Other(String),
}

impl From<MigrateError> for MigrationFailure {
    fn from(e: MigrateError) -> Self {
        match e {
            MigrateError::ExecuteMigration(err, version) => Self::Failed {
                version,
                message: err.to_string(),
            },
            MigrateError::VersionMissing(version) => Self::Newer(version),
            other => Self::Other(other.to_string()),
        }
    }
}

pub async fn run(owner: &PgPool) -> Result<(), MigrationFailure> {
    run_with(&MIGRATOR, owner).await
}

pub async fn run_with(migrator: &Migrator, owner: &PgPool) -> Result<(), MigrationFailure> {
    migrator.run(owner).await.map_err(Into::into)
}

/// (applied, pending) migration counts, for the Instance page and `/readyz`.
pub async fn status(pool: &PgPool) -> sqlx::Result<(usize, usize)> {
    status_of(&MIGRATOR, pool).await
}

pub async fn status_of(migrator: &Migrator, pool: &PgPool) -> sqlx::Result<(usize, usize)> {
    let applied: Vec<i64> =
        sqlx::query_scalar("SELECT version FROM _sqlx_migrations WHERE success")
            .fetch_all(pool)
            .await
            .unwrap_or_default();
    let known: Vec<i64> = migrator
        .iter()
        .filter(|m| m.migration_type.is_up_migration())
        .map(|m| m.version)
        .collect();
    let pending = known.iter().filter(|v| !applied.contains(v)).count();
    Ok((known.len() - pending, pending))
}
