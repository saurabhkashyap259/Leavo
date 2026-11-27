//! Test support: one PostgreSQL container per test binary, one fresh migrated database per test.
//!
//! Set `LEAVO_TEST_DATABASE_URL=postgres://user:pass@host:port` (no database name) to reuse an
//! existing server instead of starting a container.
use crate::{migrate, pool};
use sqlx::PgPool;
use std::sync::OnceLock;
use testcontainers::runners::AsyncRunner;
use testcontainers::ImageExt;
use testcontainers_modules::postgres::Postgres;
use uuid::Uuid;

static SERVER_URL: OnceLock<String> = OnceLock::new();
/// Id of the container this process started, removed when the process exits.
static CONTAINER_ID: OnceLock<String> = OnceLock::new();

/// The test harness ends with `process::exit`, which runs exit hooks but never drops the
/// container (it lives on a parked thread), and Rust testcontainers has no reaper by default.
/// Without this every test binary run would leave a PostgreSQL container running.
/// `unsafe`: it runs after `main`; it only spawns `docker rm` and touches no Rust runtime state.
#[dtor::dtor(unsafe)]
fn remove_container() {
    if let Some(id) = CONTAINER_ID.get() {
        let _ = std::process::Command::new("docker")
            .args(["rm", "--force", "--volumes", id])
            .output();
    }
}

fn server_url() -> &'static str {
    SERVER_URL.get_or_init(|| {
        if let Ok(url) = std::env::var("LEAVO_TEST_DATABASE_URL") {
            return url.trim_end_matches('/').to_string();
        }
        // The container lives on its own runtime thread for the whole test process, so it
        // outlives the per-test Tokio runtimes; `remove_container` deletes it on exit.
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("test runtime");
            rt.block_on(async move {
                let container = Postgres::default()
                    .with_tag("alpine")
                    .start()
                    .await
                    .expect("start PostgreSQL test container (is Docker running?)");
                CONTAINER_ID.set(container.id().to_string()).ok();
                let host = container.get_host().await.expect("container host");
                let port = container
                    .get_host_port_ipv4(5432)
                    .await
                    .expect("container port");
                tx.send(format!("postgres://postgres:postgres@{host}:{port}"))
                    .ok();
                std::future::pending::<()>().await;
                drop(container);
            });
        });
        rx.recv().expect("PostgreSQL test container did not start")
    })
}

/// A new, empty database (no migrations) and an owner pool for it.
pub async fn empty_database() -> (String, PgPool) {
    let server = server_url();
    let admin = PgPool::connect(&format!("{server}/postgres"))
        .await
        .expect("connect admin");
    let name = format!("t_{}", Uuid::now_v7().simple());
    sqlx::query(&format!("CREATE DATABASE {name}"))
        .execute(&admin)
        .await
        .expect("create db");
    admin.close().await;
    let url = format!("{server}/{name}");
    let owner = pool::connect_owner(&url, 2).await.expect("owner pool");
    (url, owner)
}

/// A freshly migrated database with an owner pool and an RLS-enforced app pool.
pub struct TestDb {
    pub name: String,
    pub url: String,
    pub owner: PgPool,
    pub app: PgPool,
}

impl TestDb {
    pub async fn new() -> Self {
        let server = server_url();
        let admin = PgPool::connect(&format!("{server}/postgres"))
            .await
            .expect("connect admin");
        let name = format!("t_{}", Uuid::now_v7().simple());
        sqlx::query(&format!("CREATE DATABASE {name}"))
            .execute(&admin)
            .await
            .expect("create db");
        admin.close().await;

        let url = format!("{server}/{name}");
        let owner = pool::connect_owner(&url, 5).await.expect("owner pool");
        migrate::run(&owner).await.expect("migrations");
        let app = pool::connect_app(&url, 10).await.expect("app pool");
        Self {
            name,
            url,
            owner,
            app,
        }
    }
}
