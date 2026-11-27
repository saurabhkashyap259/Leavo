//! Prometheus metrics (T212, FR-065, contracts/configuration.md "Logs, metrics and alerts").
//!
//! A small registry per instance (no process-wide recorder, so tests can run many instances in
//! one process). Request counts and latency are recorded by a middleware; sign-in codes by the
//! auth routes; queue, email-failure and migration figures are read from the database at scrape
//! time. `/metrics` requires `Authorization: Bearer $LEAVO_METRICS_TOKEN` when that is set.
use crate::app::AppState;
use axum::extract::{Request, State};
use axum::http::{header, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use std::fmt::Write;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;
use subtle::ConstantTimeEq;

/// Upper bounds of the latency histogram, seconds.
const BUCKETS: [f64; 10] = [0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0];
/// Status classes counted separately.
const CLASSES: [&str; 5] = ["1xx", "2xx", "3xx", "4xx", "5xx"];

#[derive(Default)]
pub struct Metrics {
    requests: [AtomicU64; 5],
    /// Cumulative counts per bucket, then +Inf.
    buckets: [AtomicU64; 11],
    /// Sum of durations in microseconds.
    duration_micros: AtomicU64,
    codes_sent: AtomicU64,
    codes_verified: AtomicU64,
}

impl Metrics {
    pub fn code_sent(&self) {
        self.codes_sent.fetch_add(1, Ordering::Relaxed);
    }

    pub fn code_verified(&self) {
        self.codes_verified.fetch_add(1, Ordering::Relaxed);
    }

    fn observe(&self, status: u16, seconds: f64) {
        let class = usize::from(status / 100).clamp(1, 5) - 1;
        self.requests[class].fetch_add(1, Ordering::Relaxed);
        for (i, upper) in BUCKETS.iter().enumerate() {
            if seconds <= *upper {
                self.buckets[i].fetch_add(1, Ordering::Relaxed);
            }
        }
        self.buckets[BUCKETS.len()].fetch_add(1, Ordering::Relaxed);
        self.duration_micros
            .fetch_add((seconds * 1e6) as u64, Ordering::Relaxed);
    }
}

/// Count and time every request (except scrapes of `/metrics` itself).
pub async fn layer(State(state): State<AppState>, req: Request, next: Next) -> Response {
    let scrape = req.uri().path() == "/metrics";
    let started = Instant::now();
    let resp = next.run(req).await;
    if !scrape {
        state
            .metrics
            .observe(resp.status().as_u16(), started.elapsed().as_secs_f64());
    }
    resp
}

fn authorized(state: &AppState, headers: &axum::http::HeaderMap) -> bool {
    let Some(expected) = state.config.metrics_token.as_deref() else {
        return true;
    };
    let given = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .unwrap_or("");
    given.as_bytes().ct_eq(expected.as_bytes()).into()
}

pub async fn scrape(State(state): State<AppState>, headers: axum::http::HeaderMap) -> Response {
    if !authorized(&state, &headers) {
        return (
            StatusCode::UNAUTHORIZED,
            [(header::WWW_AUTHENTICATE, "Bearer")],
            "unauthorized\n",
        )
            .into_response();
    }
    let now = state.clock.now();
    let (queued, oldest): (i64, Option<f64>) = sqlx::query_as(
        "SELECT count(*), extract(epoch FROM ($1 - min(run_at)))::float8
         FROM jobs WHERE status = 'queued' AND run_at <= $1",
    )
    .bind(now)
    .fetch_one(&state.owner)
    .await
    .unwrap_or((0, None));
    let email_failures: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM jobs WHERE kind = 'send_email' AND last_error IS NOT NULL",
    )
    .fetch_one(&state.owner)
    .await
    .unwrap_or(0);
    let pending = leavo_db::migrate::status(&state.owner)
        .await
        .map(|(_, p)| p)
        .unwrap_or(0);

    let m = &state.metrics;
    let mut out = String::new();
    let _ = writeln!(
        out,
        "# HELP leavo_http_requests_total HTTP requests by status class."
    );
    let _ = writeln!(out, "# TYPE leavo_http_requests_total counter");
    for (i, class) in CLASSES.iter().enumerate() {
        let _ = writeln!(
            out,
            "leavo_http_requests_total{{status=\"{class}\"}} {}",
            m.requests[i].load(Ordering::Relaxed)
        );
    }
    let _ = writeln!(
        out,
        "# HELP leavo_http_request_duration_seconds HTTP request latency."
    );
    let _ = writeln!(out, "# TYPE leavo_http_request_duration_seconds histogram");
    for (i, upper) in BUCKETS.iter().enumerate() {
        let _ = writeln!(
            out,
            "leavo_http_request_duration_seconds_bucket{{le=\"{upper}\"}} {}",
            m.buckets[i].load(Ordering::Relaxed)
        );
    }
    let total = m.buckets[BUCKETS.len()].load(Ordering::Relaxed);
    let _ = writeln!(
        out,
        "leavo_http_request_duration_seconds_bucket{{le=\"+Inf\"}} {total}"
    );
    let _ = writeln!(
        out,
        "leavo_http_request_duration_seconds_sum {}",
        m.duration_micros.load(Ordering::Relaxed) as f64 / 1e6
    );
    let _ = writeln!(out, "leavo_http_request_duration_seconds_count {total}");
    for (name, help, value) in [
        (
            "leavo_login_codes_sent_total",
            "Sign-in codes sent.",
            m.codes_sent.load(Ordering::Relaxed),
        ),
        (
            "leavo_login_codes_verified_total",
            "Sign-in codes verified.",
            m.codes_verified.load(Ordering::Relaxed),
        ),
        (
            "leavo_email_send_failures_total",
            "Email deliveries that failed at least once.",
            email_failures.max(0) as u64,
        ),
    ] {
        let _ = writeln!(
            out,
            "# HELP {name} {help}\n# TYPE {name} counter\n{name} {value}"
        );
    }
    for (name, help, value) in [
        (
            "leavo_jobs_queued",
            "Jobs due and waiting to run.",
            queued as f64,
        ),
        (
            "leavo_job_oldest_queued_seconds",
            "Age of the oldest job due.",
            oldest.unwrap_or(0.0).max(0.0),
        ),
        (
            "leavo_migrations_pending",
            "Migrations not yet applied.",
            pending as f64,
        ),
    ] {
        let _ = writeln!(
            out,
            "# HELP {name} {help}\n# TYPE {name} gauge\n{name} {value}"
        );
    }
    (
        StatusCode::OK,
        [(
            header::CONTENT_TYPE,
            "text/plain; version=0.0.4; charset=utf-8",
        )],
        out,
    )
        .into_response()
}
