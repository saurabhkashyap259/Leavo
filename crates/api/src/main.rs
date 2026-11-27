//! `leavo` — one binary for the web server, the job worker and operator commands
//! (contracts/configuration.md).
use clap::{Parser, Subcommand};
use leavo_api::app::{build_runner, build_scheduler, router, AppState};
use leavo_api::config::{Config, StorageConfig};
use leavo_core::clock::SystemClock;
use leavo_email::{SmtpSender, Templates};
use leavo_storage::{S3Settings, Storage};
use std::net::SocketAddr;
use std::process::ExitCode;
use std::sync::Arc;

const OPENAPI_PATH: &str = "specs/001-leave-management-v1/contracts/openapi.yaml";

#[derive(Parser)]
#[command(
    name = "leavo",
    version,
    about = "Open-source, self-hosted leave management"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Run the web server (migrates first; runs jobs in-process when LEAVO_RUN_JOBS=true).
    Serve,
    /// Run only the background job worker and scheduler.
    Worker,
    /// Apply pending database migrations and exit.
    Migrate,
    /// Print the OpenAPI document (use --write to update the committed contract).
    Openapi {
        #[arg(long)]
        write: bool,
    },
    /// Regenerate and print the one-time setup token (only before setup completes).
    SetupToken,
    /// Print a one-time 15-minute sign-in link for an active org admin (email outage recovery).
    RecoveryLogin {
        /// Email address of an active Org Admin.
        email: String,
    },
}

#[tokio::main]
async fn main() -> ExitCode {
    let cli = Cli::parse();
    let result = match cli.command {
        Command::Openapi { write } => openapi(write),
        Command::Serve => serve().await,
        Command::Worker => worker().await,
        Command::Migrate => migrate().await,
        Command::SetupToken => setup_token().await,
        Command::RecoveryLogin { email } => recovery_login(&email).await,
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

type AnyResult = Result<(), Box<dyn std::error::Error>>;

fn openapi(write: bool) -> AnyResult {
    let yaml = leavo_api::openapi::openapi_yaml();
    if write {
        std::fs::write(OPENAPI_PATH, &yaml)?;
        eprintln!("wrote {OPENAPI_PATH}");
    } else {
        print!("{yaml}");
    }
    Ok(())
}

fn load_config() -> Result<Config, Box<dyn std::error::Error>> {
    Ok(Config::from_env()?)
}

async fn connect(
    config: &Config,
) -> Result<(sqlx::PgPool, sqlx::PgPool), Box<dyn std::error::Error>> {
    let owner = leavo_db::pool::connect_owner(&config.database_url, 2).await?;
    leavo_db::migrate::run(&owner)
        .await
        .map_err(|e| e.to_string())?;
    leavo_api::startup::apply_secret_rotation(&owner, &config.secret_key, chrono::Utc::now())
        .await?;
    let app =
        leavo_db::pool::connect_app(&config.database_url, config.database_max_connections).await?;
    Ok((owner, app))
}

fn state(
    config: Config,
    owner: sqlx::PgPool,
    app: sqlx::PgPool,
) -> Result<AppState, Box<dyn std::error::Error>> {
    let email = SmtpSender::from_url(&config.smtp_url, &config.email_from)?;
    let storage = match &config.storage {
        StorageConfig::Local { path } => Storage::local(path)?,
        StorageConfig::S3 {
            endpoint,
            region,
            bucket,
            access_key,
            secret_key,
        } => Storage::s3(&S3Settings {
            endpoint: endpoint.clone(),
            region: region.clone(),
            bucket: bucket.clone(),
            access_key: access_key.clone(),
            secret_key: secret_key.clone(),
        })?,
    };
    Ok(AppState::new(
        Arc::new(config),
        app,
        owner,
        Arc::new(SystemClock),
        Arc::new(email),
        Arc::new(Templates::new()),
        storage,
    ))
}

async fn migrate() -> AnyResult {
    let config = load_config()?;
    leavo_api::telemetry::init(&config.log_level);
    let owner = leavo_db::pool::connect_owner(&config.database_url, 2).await?;
    leavo_db::migrate::run(&owner)
        .await
        .map_err(|e| e.to_string())?;
    tracing::info!("migrations applied");
    Ok(())
}

/// Print a fresh setup token to stdout (the operator lost the one from the log).
async fn setup_token() -> AnyResult {
    let config = load_config()?;
    let (_owner, app) = connect(&config).await?;
    let token = leavo_api::setup::token::regenerate(&app).await?;
    println!("{token}");
    Ok(())
}

/// Print a one-time recovery link for an active Org Admin (FR-067).
async fn recovery_login(email: &str) -> AnyResult {
    let config = load_config()?;
    let (owner, _app) = connect(&config).await?;
    let link = leavo_api::auth::recovery::issue(
        &owner,
        config.secret_key.as_bytes(),
        &config.base_url,
        email,
        chrono::Utc::now(),
    )
    .await?;
    println!("Sign-in link for {email} (valid 15 minutes, works once):");
    println!("{link}");
    Ok(())
}

async fn serve() -> AnyResult {
    let config = load_config()?;
    leavo_api::telemetry::init(&config.log_level);
    let bind = config.bind;
    let run_jobs = config.run_jobs;
    let (owner, app) = connect(&config).await?;
    match leavo_api::setup::token::ensure(&app).await? {
        // Shown once, only to whoever can read the server log (FR-007). Not a tracing field,
        // so log redaction does not hide it; the token is useless once setup completes.
        Some(token) => eprintln!(
            "\n  Setup token: {token}\n  Open {}/setup to create your organization.\n",
            config.base_url
        ),
        None => {
            if leavo_api::setup::token::setup_required(&app).await? {
                eprintln!("  Setup is pending. Run `leavo setup-token` to get a new setup token.");
            }
        }
    }
    let state = state(config, owner, app)?;
    if run_jobs {
        spawn_background(state.clone());
    }
    let listener = tokio::net::TcpListener::bind(bind).await?;
    tracing::info!(%bind, version = env!("CARGO_PKG_VERSION"), "leavo listening");
    axum::serve(
        listener,
        router(state).into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown_signal())
    .await?;
    Ok(())
}

async fn worker() -> AnyResult {
    let config = load_config()?;
    leavo_api::telemetry::init(&config.log_level);
    let (owner, app) = connect(&config).await?;
    let state = state(config, owner, app)?;
    let runner = build_runner(&state, &worker_id());
    let scheduler = build_scheduler(&state);
    tracing::info!("leavo worker started");
    tokio::join!(
        runner.run_until(shutdown_signal()),
        scheduler.run_until(shutdown_signal())
    );
    Ok(())
}

fn spawn_background(state: AppState) {
    let runner = build_runner(&state, &worker_id());
    let scheduler = build_scheduler(&state);
    tokio::spawn(async move { runner.run_until(std::future::pending()).await });
    tokio::spawn(async move { scheduler.run_until(std::future::pending()).await });
}

fn worker_id() -> String {
    format!(
        "{}-{}",
        std::env::var("HOSTNAME").unwrap_or_else(|_| "leavo".into()),
        std::process::id()
    )
}

async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    #[cfg(unix)]
    let term = async {
        if let Ok(mut s) = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        {
            s.recv().await;
        }
    };
    #[cfg(not(unix))]
    let term = std::future::pending::<()>();
    tokio::select! { _ = ctrl_c => {}, _ = term => {} }
}
