//! Application state, router and background-worker wiring.
use crate::config::Config;
use crate::error::ApiError;
use crate::{middleware, routes, spa};
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::get;
use axum::Router;
use leavo_core::clock::Clock;
use leavo_core::error::ErrorCode;
use leavo_email::{EmailSender, Templates};
use leavo_storage::Storage;
use leavo_worker::jobs::approval_reminder::{ApprovalReminder, ApprovalReminders};
use leavo_worker::jobs::comp_off_expiry::{CompOffExpiries, CompOffExpiry};
use leavo_worker::jobs::send_email::SendEmail;
use leavo_worker::jobs::update_check::{ReleaseSource, UpdateCheck, UpdateChecks};
use leavo_worker::jobs::year_end_rollover::{YearEndRollover, YearEndRollovers};
use leavo_worker::runner::Runner;
use leavo_worker::scheduler::Scheduler;
use sqlx::PgPool;
use std::sync::Arc;
use tower_http::request_id::{MakeRequestUuid, PropagateRequestIdLayer, SetRequestIdLayer};
use tower_http::trace::TraceLayer;

/// Shared, cheap-to-clone application state.
#[derive(Clone)]
pub struct AppState {
    pub config: Arc<Config>,
    /// RLS-enforced pool (`SET ROLE leavo_app`). All request handling uses this.
    pub db: PgPool,
    /// Owner pool: migrations and migration status only.
    pub owner: PgPool,
    pub clock: Arc<dyn Clock>,
    pub email: Arc<dyn EmailSender>,
    pub templates: Arc<Templates>,
    /// Attachment files (local disk, S3 or memory).
    pub storage: Storage,
    /// Prometheus counters for `/metrics`.
    pub metrics: Arc<crate::metrics::Metrics>,
    /// Where the daily update check looks for the latest release.
    pub releases: Arc<dyn ReleaseSource>,
}

impl AppState {
    pub fn new(
        config: Arc<Config>,
        db: PgPool,
        owner: PgPool,
        clock: Arc<dyn Clock>,
        email: Arc<dyn EmailSender>,
        templates: Arc<Templates>,
        storage: Storage,
    ) -> Self {
        Self {
            config,
            db,
            owner,
            clock,
            email,
            templates,
            storage,
            metrics: Arc::new(crate::metrics::Metrics::default()),
            releases: Arc::new(crate::releases::GitHubReleases),
        }
    }

    /// Use another release source (tests, offline builds).
    pub fn with_releases(mut self, releases: Arc<dyn ReleaseSource>) -> Self {
        self.releases = releases;
        self
    }
}

pub fn router(state: AppState) -> Router {
    let api = routes::api_router().fallback(|| async { ApiError::new(ErrorCode::NotFound) });
    Router::new()
        .route("/healthz", get(|| async { "ok" }))
        .route("/readyz", get(readyz))
        .route("/metrics", get(crate::metrics::scrape))
        .nest("/api/v1", api)
        .fallback(spa::serve)
        .layer(axum::middleware::from_fn(middleware::deprecation::layer))
        .layer(axum::middleware::from_fn(middleware::csrf::layer))
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            crate::metrics::layer,
        ))
        .layer(TraceLayer::new_for_http().make_span_with(crate::telemetry::request_span))
        .layer(PropagateRequestIdLayer::x_request_id())
        .layer(SetRequestIdLayer::x_request_id(MakeRequestUuid))
        .with_state(state)
}

/// Ready when the database answers and no migrations are pending (FR-069).
async fn readyz(State(state): State<AppState>) -> impl IntoResponse {
    let db_ok = sqlx::query("SELECT 1").execute(&state.db).await.is_ok();
    let pending = leavo_db::migrate::status(&state.owner)
        .await
        .map(|(_, p)| p)
        .unwrap_or(usize::MAX);
    if db_ok && pending == 0 {
        (StatusCode::OK, "ready")
    } else {
        (StatusCode::SERVICE_UNAVAILABLE, "not ready")
    }
}

/// A job runner with every handler registered.
pub fn build_runner(state: &AppState, worker_id: &str) -> Runner {
    let mut runner = Runner::new(state.db.clone(), state.clock.clone(), worker_id);
    runner.register(
        SendEmail::KIND,
        Arc::new(SendEmail::new(state.templates.clone(), state.email.clone())),
    );
    runner.register(
        ApprovalReminder::KIND,
        Arc::new(ApprovalReminder::new(
            state.db.clone(),
            state.clock.clone(),
            state.config.base_url.clone(),
        )),
    );
    runner.register(
        CompOffExpiry::KIND,
        Arc::new(CompOffExpiry::new(
            state.db.clone(),
            state.clock.clone(),
            state.config.base_url.clone(),
        )),
    );
    runner.register(
        YearEndRollover::KIND,
        Arc::new(YearEndRollover::new(
            state.db.clone(),
            state.clock.clone(),
            state.config.base_url.clone(),
        )),
    );
    runner.register(
        UpdateCheck::KIND,
        Arc::new(UpdateCheck::new(
            state.db.clone(),
            state.clock.clone(),
            state.releases.clone(),
        )),
    );
    runner
}

/// The scheduler with every periodic job registered.
pub fn build_scheduler(state: &AppState) -> Scheduler {
    let mut scheduler = Scheduler::new(state.db.clone(), state.clock.clone());
    scheduler.register(Arc::new(ApprovalReminders));
    scheduler.register(Arc::new(CompOffExpiries));
    scheduler.register(Arc::new(YearEndRollovers));
    // FR-052: with LEAVO_UPDATE_CHECK=false the check is never even scheduled.
    if state.config.update_check {
        scheduler.register(Arc::new(UpdateChecks));
    }
    scheduler
}
