//! T036 — integration test harness: real router on an ephemeral port, fresh database per test,
//! fixed clock, in-memory email. Shared by every `crates/api/tests/*.rs`.
#![allow(dead_code)]
pub mod factories;

use chrono::{DateTime, TimeZone, Utc};
use leavo_api::app::{router, AppState};
use leavo_api::config::Config;
use leavo_core::clock::FixedClock;
use leavo_db::testkit::TestDb;
use leavo_email::{MemoryEmailSender, Templates};
use leavo_storage::Storage;
use leavo_worker::runner::Runner;
use reqwest::header::{HeaderMap, HeaderValue, COOKIE};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;

pub const SECRET: &str = "test-secret-test-secret-test-secret-test-secret-test-secret-0000";

pub fn t0() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 11, 24, 9, 0, 0).unwrap()
}

pub fn test_config(db_url: &str, extra: &[(&str, &str)]) -> Config {
    let mut env: HashMap<String, String> = [
        ("DATABASE_URL", db_url),
        ("LEAVO_BASE_URL", "http://leavo.test"),
        ("LEAVO_SECRET_KEY", SECRET),
        ("SMTP_URL", "smtp://localhost:1025"),
        ("EMAIL_FROM", "Leavo <leave@leavo.test>"),
        ("LEAVO_UPDATE_CHECK", "false"),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v.to_string()))
    .collect();
    for (k, v) in extra {
        env.insert(k.to_string(), v.to_string());
    }
    Config::from_map(&env).expect("test config")
}

pub struct TestApp {
    pub addr: SocketAddr,
    pub db: TestDb,
    pub clock: Arc<FixedClock>,
    pub email: Arc<MemoryEmailSender>,
    /// The release source the update check asks (never the network in tests).
    pub releases: Arc<leavo_worker::jobs::update_check::StaticRelease>,
    pub state: AppState,
}

impl TestApp {
    pub async fn spawn() -> Self {
        Self::spawn_with(&[]).await
    }

    pub async fn spawn_with(extra_env: &[(&str, &str)]) -> Self {
        // Debugging aid: LEAVO_TEST_LOG=1 prints the server's JSON logs.
        if std::env::var_os("LEAVO_TEST_LOG").is_some() {
            leavo_api::telemetry::init("debug");
        }
        let db = TestDb::new().await;
        let clock = Arc::new(FixedClock::new(t0()));
        let email = Arc::new(MemoryEmailSender::default());
        let state = AppState::new(
            Arc::new(test_config(&db.url, extra_env)),
            db.app.clone(),
            db.owner.clone(),
            clock.clone(),
            email.clone(),
            Arc::new(Templates::new()),
            Storage::memory(),
        );
        let releases = Arc::new(leavo_worker::jobs::update_check::StaticRelease::new(
            "99.1.0",
        ));
        let state = state.with_releases(releases.clone());
        let app = router(state.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(
                listener,
                app.into_make_service_with_connect_info::<SocketAddr>(),
            )
            .await
            .unwrap();
        });
        Self {
            addr,
            db,
            clock,
            email,
            releases,
            state,
        }
    }

    pub fn url(&self, path: &str) -> String {
        format!("http://{}{}", self.addr, path)
    }

    /// A client that sends the CSRF header but no session.
    pub fn anon(&self) -> reqwest::Client {
        client_with(HeaderMap::new())
    }

    /// A client authenticated with a raw session token.
    pub fn with_token(&self, token: &str) -> reqwest::Client {
        let mut h = HeaderMap::new();
        h.insert(
            COOKIE,
            HeaderValue::from_str(&format!("leavo_sid={token}")).unwrap(),
        );
        client_with(h)
    }

    /// Create a session for `user` directly and return an authenticated client.
    pub async fn login_as(&self, user: &factories::User) -> reqwest::Client {
        let token = factories::create_session(self, user, false).await;
        self.with_token(&token)
    }

    /// Enqueue whatever the scheduler finds due now, then run every job.
    pub async fn tick(&self) -> usize {
        leavo_api::app::build_scheduler(&self.state)
            .tick()
            .await
            .unwrap();
        self.run_jobs().await
    }

    /// Run all due background jobs (emails etc.).
    pub async fn run_jobs(&self) -> usize {
        let runner: Runner = leavo_api::app::build_runner(&self.state, "test-worker");
        let mut total = 0;
        loop {
            let n = runner.run_once(50).await.unwrap();
            if n == 0 {
                break total;
            }
            total += n;
        }
    }
}

fn client_with(mut headers: HeaderMap) -> reqwest::Client {
    headers.insert("x-requested-with", HeaderValue::from_static("leavo"));
    reqwest::Client::builder()
        .default_headers(headers)
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap()
}

/// Run queued jobs and return the 6-digit codes emailed to `to`, oldest first.
pub async fn codes_sent_to(app: &TestApp, to: &str) -> Vec<String> {
    app.run_jobs().await;
    app.email
        .sent()
        .into_iter()
        .filter(|m| m.to == to)
        .filter_map(|m| m.subject.split_whitespace().next().map(str::to_string))
        .filter(|c| c.len() == 6 && c.bytes().all(|b| b.is_ascii_digit()))
        .collect()
}

pub async fn post_json(
    client: &reqwest::Client,
    url: String,
    body: serde_json::Value,
) -> (u16, serde_json::Value) {
    let res = client.post(url).json(&body).send().await.unwrap();
    let status = res.status().as_u16();
    let text = res.text().await.unwrap();
    (
        status,
        serde_json::from_str(&text).unwrap_or(serde_json::Value::Null),
    )
}

pub async fn get_json(client: &reqwest::Client, url: String) -> (u16, serde_json::Value) {
    let res = client.get(url).send().await.unwrap();
    let status = res.status().as_u16();
    let text = res.text().await.unwrap();
    (
        status,
        serde_json::from_str(&text).unwrap_or(serde_json::Value::Null),
    )
}

pub async fn patch_json(
    client: &reqwest::Client,
    url: String,
    body: serde_json::Value,
) -> (u16, serde_json::Value) {
    let res = client.patch(url).json(&body).send().await.unwrap();
    let status = res.status().as_u16();
    let text = res.text().await.unwrap();
    (
        status,
        serde_json::from_str(&text).unwrap_or(serde_json::Value::Null),
    )
}

pub async fn delete(client: &reqwest::Client, url: String) -> (u16, serde_json::Value) {
    let res = client.delete(url).send().await.unwrap();
    let status = res.status().as_u16();
    let text = res.text().await.unwrap();
    (
        status,
        serde_json::from_str(&text).unwrap_or(serde_json::Value::Null),
    )
}

/// The `leavo_sid` token from a response's `Set-Cookie`, if any.
pub fn session_cookie(res: &reqwest::Response) -> Option<String> {
    res.headers()
        .get_all(reqwest::header::SET_COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .find_map(|c| c.strip_prefix("leavo_sid="))
        .map(|c| c.split(';').next().unwrap_or("").to_string())
        .filter(|t| !t.is_empty())
}

/// An org with an admin, signed in. Returns `(org, admin, admin client)`.
pub async fn org_with_admin(
    app: &TestApp,
    domain: &str,
) -> (factories::Org, factories::User, reqwest::Client) {
    use factories::*;
    let org = create_org(app, domain, domain).await;
    let admin = create_user(
        app,
        &org,
        NewUser::employee(&format!("admin@{domain}")).role("admin"),
    )
    .await;
    let client = app.login_as(&admin).await;
    (org, admin, client)
}

/// A leave request body with sensible defaults.
pub fn leave_body(leave_type: uuid::Uuid, from: &str, to: &str) -> serde_json::Value {
    serde_json::json!({
        "leave_type_id": leave_type,
        "from_date": from,
        "to_date": to,
        "start_half": null,
        "end_half": null,
        "reason": "Family function",
        "attachment_id": null,
        "notify": [],
    })
}

/// `POST /leaves` with an `Idempotency-Key`.
pub async fn post_leave(
    app: &TestApp,
    client: &reqwest::Client,
    body: &serde_json::Value,
    key: &str,
) -> (u16, serde_json::Value) {
    let res = client
        .post(app.url("/api/v1/leaves"))
        .header("Idempotency-Key", key)
        .json(body)
        .send()
        .await
        .unwrap();
    let status = res.status().as_u16();
    let text = res.text().await.unwrap();
    (
        status,
        serde_json::from_str(&text).unwrap_or(serde_json::Value::Null),
    )
}

/// A fresh idempotency key.
pub fn key() -> String {
    uuid::Uuid::new_v4().to_string()
}

/// The balance card of `code` from `/me/balances`.
pub async fn balance_card(
    app: &TestApp,
    client: &reqwest::Client,
    code: &str,
    year: Option<i32>,
) -> serde_json::Value {
    let url = match year {
        Some(y) => app.url(&format!("/api/v1/me/balances?year={y}")),
        None => app.url("/api/v1/me/balances"),
    };
    let (status, cards) = get_json(client, url).await;
    assert_eq!(status, 200, "{cards}");
    cards
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["leave_type"]["code"] == code)
        .cloned()
        .unwrap_or_else(|| panic!("no {code} card in {cards}"))
}

/// Submit a leave request and return its id (panics unless 201).
pub async fn submit(app: &TestApp, client: &reqwest::Client, body: &serde_json::Value) -> String {
    let (status, out) = post_leave(app, client, body, &key()).await;
    assert_eq!(status, 201, "{out}");
    out["id"].as_str().unwrap().to_string()
}

/// `POST /approvals/leave/{id}/decide` with a fresh `Idempotency-Key`.
pub async fn decide(
    app: &TestApp,
    client: &reqwest::Client,
    id: &str,
    body: serde_json::Value,
) -> (u16, serde_json::Value) {
    decide_with_key(app, client, id, body, &key()).await
}

pub async fn decide_with_key(
    app: &TestApp,
    client: &reqwest::Client,
    id: &str,
    body: serde_json::Value,
    k: &str,
) -> (u16, serde_json::Value) {
    let res = client
        .post(app.url(&format!("/api/v1/approvals/leave/{id}/decide")))
        .header("Idempotency-Key", k)
        .json(&body)
        .send()
        .await
        .unwrap();
    let status = res.status().as_u16();
    let text = res.text().await.unwrap();
    (
        status,
        serde_json::from_str(&text).unwrap_or(serde_json::Value::Null),
    )
}

pub fn approve() -> serde_json::Value {
    serde_json::json!({ "decision": "approve", "version": 1 })
}

/// `POST /comp-offs` with a fresh `Idempotency-Key`.
pub async fn post_comp_off(
    app: &TestApp,
    client: &reqwest::Client,
    body: &serde_json::Value,
) -> (u16, serde_json::Value) {
    let res = client
        .post(app.url("/api/v1/comp-offs"))
        .header("Idempotency-Key", key())
        .json(body)
        .send()
        .await
        .unwrap();
    let status = res.status().as_u16();
    let text = res.text().await.unwrap();
    (
        status,
        serde_json::from_str(&text).unwrap_or(serde_json::Value::Null),
    )
}

/// A comp-off request body for `(date, portion)` pairs.
pub fn comp_off_body(dates: &[(&str, f64)]) -> serde_json::Value {
    let dates: Vec<serde_json::Value> = dates
        .iter()
        .map(|(d, p)| serde_json::json!({ "date": d, "portion": p }))
        .collect();
    serde_json::json!({ "dates": dates, "reason": "Release weekend", "notify": [] })
}

/// `POST /approvals/{kind}/{id}/decide` with a fresh `Idempotency-Key`.
pub async fn decide_kind(
    app: &TestApp,
    client: &reqwest::Client,
    kind: &str,
    id: &str,
    body: serde_json::Value,
) -> (u16, serde_json::Value) {
    let res = client
        .post(app.url(&format!("/api/v1/approvals/{kind}/{id}/decide")))
        .header("Idempotency-Key", key())
        .json(&body)
        .send()
        .await
        .unwrap();
    let status = res.status().as_u16();
    let text = res.text().await.unwrap();
    (
        status,
        serde_json::from_str(&text).unwrap_or(serde_json::Value::Null),
    )
}

/// Request comp-off for `dates` and have `approver` approve it. Returns the request id.
pub async fn earn_comp_off(
    app: &TestApp,
    employee: &reqwest::Client,
    approver: &reqwest::Client,
    dates: &[(&str, f64)],
) -> String {
    let (status, out) = post_comp_off(app, employee, &comp_off_body(dates)).await;
    assert_eq!(status, 201, "{out}");
    let id = out["id"].as_str().unwrap().to_string();
    let (status, out) = decide_kind(app, approver, "comp_off", &id, approve()).await;
    assert_eq!(status, 200, "{out}");
    id
}

/// `GET /comp-offs` for the signed-in user.
pub async fn comp_offs(app: &TestApp, client: &reqwest::Client) -> serde_json::Value {
    let (status, out) = get_json(client, app.url("/api/v1/comp-offs")).await;
    assert_eq!(status, 200, "{out}");
    out
}
