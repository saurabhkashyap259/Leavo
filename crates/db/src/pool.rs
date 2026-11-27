//! Connection pools.
//!
//! * The **owner** pool connects as the `DATABASE_URL` user. It runs migrations only.
//! * The **app** pool switches every connection to `leavo_app` (`SET ROLE`), a non-owner,
//!   non-superuser role, so Row-Level Security always applies (constitution IV).
use sqlx::postgres::PgPoolOptions;
use sqlx::{Executor, PgPool};
use std::time::Duration;

pub const APP_ROLE: &str = "leavo_app";

pub async fn connect_owner(url: &str, max_connections: u32) -> sqlx::Result<PgPool> {
    PgPoolOptions::new()
        .max_connections(max_connections.max(1))
        .acquire_timeout(Duration::from_secs(10))
        .connect(url)
        .await
}

pub async fn connect_app(url: &str, max_connections: u32) -> sqlx::Result<PgPool> {
    PgPoolOptions::new()
        .max_connections(max_connections.max(1))
        .acquire_timeout(Duration::from_secs(10))
        .after_connect(|conn, _meta| {
            Box::pin(async move {
                conn.execute("SET ROLE leavo_app").await?;
                Ok(())
            })
        })
        .connect(url)
        .await
}
