//! `/auth/code`, `/auth/verify`, `/auth/logout` — passwordless sign-in (US1, FR-001–FR-006).
use crate::app::AppState;
use crate::auth::extractor::COOKIE;
use crate::auth::rate_limit::{self, CODE_PER_EMAIL, CODE_PER_IP};
use crate::auth::CurrentUser;
use crate::config::InstanceMode;
use crate::error::{field_error, ApiError, ApiResult};
use crate::extract::ApiJson;
use crate::setup;
use axum::extract::{ConnectInfo, Query, State};
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use chrono::{Duration, NaiveDate};
use leavo_core::domains::normalize_domain;
use leavo_core::error::ErrorCode;
use leavo_core::otp::{self, CODE_TTL_MINUTES, MAX_WRONG_ATTEMPTS};
use leavo_core::tokens::{new_session_token, sign_ticket, SESSION_MAX};
use leavo_db::audit::{write_audit, AuditEntry};
use leavo_db::outbox::{notify, OutgoingEmail};
use leavo_db::users_repo::{self, NewUserRow};
use leavo_db::{auth_repo, sessions, tx};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::net::SocketAddr;
use utoipa::ToSchema;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum CodePurpose {
    Login,
    Signup,
    /// Multi-org mode: verify a founder's email before `POST /orgs` (FR-008).
    RegisterOrg,
    /// First-run setup: verify the first admin's email (FR-007); needs `setup_token`.
    Setup,
}

impl CodePurpose {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Login => "login",
            Self::Signup => "signup",
            Self::RegisterOrg => "register_org",
            Self::Setup => "setup",
        }
    }
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct CodeRequest {
    pub email: String,
    pub purpose: CodePurpose,
    /// Required for signup.
    pub full_name: Option<String>,
    /// Optional for signup; used to pro-rate quotas (FR-013, FR-064).
    pub date_of_joining: Option<NaiveDate>,
    /// Required for purpose `setup`.
    pub setup_token: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct CodeSent {
    /// Always `"sent"` — the response never reveals whether an account exists (FR-003).
    pub status: &'static str,
}

#[utoipa::path(
    post, path = "/api/v1/auth/code", tag = "auth", request_body = CodeRequest,
    responses(
        (status = 202, description = "A code was sent if the email may sign in", body = CodeSent),
        (status = 403, description = "SETUP_TOKEN_INVALID (purpose setup)", body = crate::openapi::ErrorBody),
        (status = 404, description = "purpose register_org in single-organization mode", body = crate::openapi::ErrorBody),
        (status = 409, description = "DOMAIN_TAKEN (purpose register_org)", body = crate::openapi::ErrorBody),
        (status = 422, description = "VALIDATION, DOMAIN_NOT_REGISTERED or PUBLIC_DOMAIN", body = crate::openapi::ErrorBody),
        (status = 429, description = "RATE_LIMITED or LOCKED (Retry-After header)", body = crate::openapi::ErrorBody),
    )
)]
pub async fn request_code(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    ApiJson(req): ApiJson<CodeRequest>,
) -> ApiResult<(StatusCode, Json<CodeSent>)> {
    let email = otp::normalize_email(&req.email).ok_or_else(|| field_error("email"))?;
    let full_name = req
        .full_name
        .as_deref()
        .map(str::trim)
        .filter(|n| !n.is_empty());
    if req.purpose == CodePurpose::Signup && full_name.is_none_or(|n| n.chars().count() > 120) {
        return Err(field_error("full_name"));
    }

    let now = state.clock.now();
    match req.purpose {
        CodePurpose::RegisterOrg if state.config.mode != InstanceMode::Multi => {
            return Err(ApiError::new(ErrorCode::NotFound));
        }
        CodePurpose::Setup => {
            setup::token::check(&state.db, req.setup_token.as_deref().unwrap_or(""), now).await?;
        }
        _ => {}
    }
    let ip = rate_limit::client_ip(peer.ip(), &headers, &state.config.trusted_proxies).to_string();
    let mut conn = state.db.acquire().await?;
    for (bucket, key, rules) in [
        ("code_ip", ip.as_str(), CODE_PER_IP),
        ("code_email", email.as_str(), CODE_PER_EMAIL),
    ] {
        if let Err(wait) = rate_limit::check(&mut conn, bucket, key, now, rules).await? {
            return Err(ApiError::new(ErrorCode::RateLimited).retry_after(wait));
        }
    }
    rate_limit::record(&mut conn, "code_ip", &ip, now).await?;
    rate_limit::record(&mut conn, "code_email", &email, now).await?;

    let existing = auth_repo::find_user(&mut conn, &email).await?;
    let may_sign_in = existing
        .as_ref()
        .is_some_and(|u| u.status == "active" || u.status == "invited");
    match req.purpose {
        CodePurpose::Login => {
            if may_sign_in {
                issue_code(&state, &mut conn, &email, "login", json!({})).await?;
            }
        }
        CodePurpose::Signup => {
            let domain = otp::domain_of(&email).unwrap_or_default();
            if auth_repo::find_org_by_domain(&mut conn, domain)
                .await?
                .is_none()
            {
                return Err(ApiError::new(ErrorCode::DomainNotRegistered));
            }
            if existing.is_some() {
                // Already has an account: send a sign-in code instead (same response).
                if may_sign_in {
                    issue_code(&state, &mut conn, &email, "login", json!({})).await?;
                }
            } else {
                let payload =
                    json!({ "full_name": full_name, "date_of_joining": req.date_of_joining });
                issue_code(&state, &mut conn, &email, "signup", payload).await?;
            }
        }
        CodePurpose::RegisterOrg | CodePurpose::Setup => {
            let domain = otp::domain_of(&email).unwrap_or_default();
            let domain = normalize_domain(domain).map_err(ApiError::new)?;
            if auth_repo::find_org_by_domain(&mut conn, &domain)
                .await?
                .is_some()
            {
                return Err(ApiError::new(ErrorCode::DomainTaken));
            }
            issue_code(&state, &mut conn, &email, req.purpose.as_str(), json!({})).await?;
        }
    }
    Ok((StatusCode::ACCEPTED, Json(CodeSent { status: "sent" })))
}

async fn issue_code(
    state: &AppState,
    conn: &mut sqlx::PgConnection,
    email: &str,
    purpose: &str,
    payload: serde_json::Value,
) -> ApiResult<()> {
    let now = state.clock.now();
    let code = otp::generate_code();
    let hash = otp::hash_code(state.config.secret_key.as_bytes(), email, &code);
    let mut tx = sqlx::Acquire::begin(conn).await?;
    auth_repo::replace_code(
        &mut tx,
        email,
        purpose,
        &hash,
        &payload,
        now + Duration::minutes(CODE_TTL_MINUTES),
        now,
    )
    .await?;
    notify(
        &mut tx,
        &OutgoingEmail {
            org_id: None,
            template: "login_code".into(),
            to: email.to_string(),
            context: json!({ "code": code, "purpose": purpose }),
            attachments: vec![],
        },
        now,
    )
    .await?;
    tx.commit().await?;
    state.metrics.code_sent();
    Ok(())
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct VerifyRequest {
    pub email: String,
    pub code: String,
    pub purpose: CodePurpose,
    /// 30-day session instead of 7 days (FR-005).
    #[serde(default)]
    pub keep_signed_in: bool,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct SignedInUser {
    pub id: Uuid,
    pub org_id: Uuid,
    pub email: String,
    pub full_name: String,
    pub role: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct SignedIn {
    pub user: SignedInUser,
}

/// Proof of a verified email for `POST /orgs` (purpose `register_org`).
#[derive(Debug, Serialize, ToSchema)]
pub struct Verified {
    pub verified: bool,
    /// Valid for 30 minutes.
    pub ticket: String,
}

/// `login`/`signup`: the signed-in user (and a session cookie); `register_org`: a ticket.
#[derive(Debug, Serialize, ToSchema)]
#[serde(untagged)]
pub enum VerifyResponse {
    SignedIn(SignedIn),
    Verified(Verified),
}

/// Check a code for `email` and `purpose`: lock after 5 wrong tries, single use (FR-002).
/// Runs in its own transaction so attempts and consumption stick whatever the caller does next.
pub async fn check_code(
    state: &AppState,
    email: &str,
    purpose: CodePurpose,
    presented: &str,
) -> ApiResult<auth_repo::CodeRow> {
    let invalid = || ApiError::new(ErrorCode::CodeInvalid);
    let now = state.clock.now();
    let mut tx = state.db.begin().await?;
    if let Some(until) = auth_repo::locked_until(&mut tx, email, now).await? {
        let wait = (until - now).num_seconds().max(1) as u64;
        return Err(ApiError::new(ErrorCode::Locked).retry_after(wait));
    }
    let Some(code) = auth_repo::latest_live_code(&mut tx, email, purpose.as_str(), now).await?
    else {
        return Err(invalid());
    };
    let attempts = auth_repo::record_attempt(&mut tx, code.id).await?;
    if !otp::verify_code(
        state.config.secret_key.as_bytes(),
        email,
        presented,
        &code.code_hash,
    ) {
        if i32::from(attempts) >= MAX_WRONG_ATTEMPTS {
            auth_repo::consume(&mut tx, code.id, now).await?;
            auth_repo::lock(&mut tx, email, now).await?;
            tx.commit().await?;
            let lock = Duration::minutes(otp::LOCK_MINUTES).num_seconds() as u64;
            return Err(ApiError::new(ErrorCode::Locked).retry_after(lock));
        }
        tx.commit().await?;
        return Err(invalid());
    }
    auth_repo::consume(&mut tx, code.id, now).await?;
    tx.commit().await?;
    state.metrics.code_verified();
    Ok(code)
}

#[utoipa::path(
    post, path = "/api/v1/auth/verify", tag = "auth", request_body = VerifyRequest,
    responses(
        (status = 200, description = "Signed in (sets the leavo_sid cookie), or a ticket for register_org", body = VerifyResponse),
        (status = 404, description = "purpose register_org in single-organization mode", body = crate::openapi::ErrorBody),
        (status = 422, description = "CODE_INVALID", body = crate::openapi::ErrorBody),
        (status = 429, description = "LOCKED after 5 wrong codes (Retry-After header)", body = crate::openapi::ErrorBody),
    )
)]
pub async fn verify(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    ApiJson(req): ApiJson<VerifyRequest>,
) -> ApiResult<Response> {
    let invalid = || ApiError::new(ErrorCode::CodeInvalid);
    let email = otp::normalize_email(&req.email).ok_or_else(invalid)?;
    if req.purpose == CodePurpose::RegisterOrg && state.config.mode != InstanceMode::Multi {
        return Err(ApiError::new(ErrorCode::NotFound));
    }
    if req.purpose == CodePurpose::Setup {
        // The setup code is checked by `POST /setup/complete`.
        return Err(invalid());
    }
    let code = check_code(&state, &email, req.purpose, &req.code).await?;
    let now = state.clock.now();
    if req.purpose == CodePurpose::RegisterOrg {
        let ticket = sign_ticket(
            state.config.secret_key.as_bytes(),
            &email,
            CodePurpose::RegisterOrg.as_str(),
            now.timestamp(),
        );
        return Ok(Json(VerifyResponse::Verified(Verified {
            verified: true,
            ticket,
        }))
        .into_response());
    }
    let mut tx = state.db.begin().await?;
    let found = auth_repo::find_user(&mut tx, &email).await?;
    let (user_id, org_id) = match (req.purpose, found) {
        (_, Some(u)) if u.status == "active" || u.status == "invited" => {
            tx::set_org(&mut tx, u.org_id).await?;
            if u.status == "invited" {
                users_repo::activate(&mut tx, u.id).await?;
            }
            (u.id, u.org_id)
        }
        (CodePurpose::Signup, None) => {
            let domain = otp::domain_of(&email).unwrap_or_default();
            let org = auth_repo::find_org_by_domain(&mut tx, domain)
                .await?
                .ok_or_else(|| ApiError::new(ErrorCode::DomainNotRegistered))?;
            tx::set_org(&mut tx, org).await?;
            let full_name = code.payload["full_name"].as_str().unwrap_or("").to_string();
            let doj = code.payload["date_of_joining"]
                .as_str()
                .and_then(|d| d.parse::<NaiveDate>().ok());
            let id = users_repo::insert_user(
                &mut tx,
                &NewUserRow {
                    org_id: org,
                    email: &email,
                    full_name: &full_name,
                    role: "employee",
                    status: "active",
                    date_of_joining: doj,
                },
                now,
            )
            .await?;
            let allotted = users_repo::allot_on_join(&mut tx, org, id, doj, now).await?;
            write_audit(
                &mut tx,
                &AuditEntry {
                    org_id: org,
                    actor_id: Some(id),
                    action: "user.signup",
                    entity_kind: "user",
                    entity_id: Some(id),
                    before: None,
                    after: Some(json!({ "email": email, "date_of_joining": doj, "allotments": allotted.len() })),
                    ip: Some(peer.ip()),
                },
            )
            .await?;
            (id, org)
        }
        _ => return Err(invalid()),
    };

    let (full_name, role): (String, String) =
        sqlx::query_as("SELECT full_name, role::text FROM users WHERE id = $1")
            .bind(user_id)
            .fetch_one(&mut *tx)
            .await?;
    let token = start_session(
        &mut tx,
        user_id,
        org_id,
        req.keep_signed_in,
        &headers,
        peer.ip(),
        now,
    )
    .await?;
    tx.commit().await?;

    let body = VerifyResponse::SignedIn(SignedIn {
        user: SignedInUser {
            id: user_id,
            org_id,
            email,
            full_name,
            role,
        },
    });
    Ok(with_session_cookie(Json(body).into_response(), &token))
}

/// Start a session for `user` inside `tx` and return the raw cookie token.
pub async fn start_session(
    conn: &mut sqlx::PgConnection,
    user_id: Uuid,
    org_id: Uuid,
    keep_signed_in: bool,
    headers: &HeaderMap,
    ip: std::net::IpAddr,
    now: chrono::DateTime<chrono::Utc>,
) -> ApiResult<String> {
    let (token, hash) = new_session_token();
    let user_agent = headers
        .get(header::USER_AGENT)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.chars().take(300).collect());
    sessions::create(
        &mut *conn,
        &sessions::NewSession {
            user_id,
            org_id,
            token_hash: hash.to_vec(),
            keep_signed_in,
            user_agent,
            ip: Some(ip),
        },
        now,
    )
    .await?;
    Ok(token)
}

pub fn with_session_cookie(mut resp: Response, token: &str) -> Response {
    resp.headers_mut().insert(
        header::SET_COOKIE,
        session_cookie(token, Duration::days(SESSION_MAX).num_seconds()),
    );
    resp
}

/// The cookie lives up to the 90-day cap; the database decides the real (sliding) expiry.
fn session_cookie(token: &str, max_age: i64) -> HeaderValue {
    HeaderValue::from_str(&format!(
        "{COOKIE}={token}; HttpOnly; Secure; SameSite=Lax; Path=/; Max-Age={max_age}"
    ))
    .expect("cookie header is ASCII")
}

#[derive(Debug, Deserialize)]
pub struct LogoutQuery {
    #[serde(default)]
    pub all: bool,
}

#[utoipa::path(
    post, path = "/api/v1/auth/logout", tag = "auth",
    params(("all" = Option<bool>, Query, description = "Sign out of every device")),
    responses(
        (status = 204, description = "Signed out; clears the cookie"),
        (status = 401, description = "Not signed in", body = crate::openapi::ErrorBody),
    )
)]
pub async fn logout(
    State(state): State<AppState>,
    user: CurrentUser,
    Query(q): Query<LogoutQuery>,
) -> ApiResult<Response> {
    let now = state.clock.now();
    if q.all {
        sessions::revoke_all_for_user(&state.db, user.id, now).await?;
    } else {
        sessions::revoke(&state.db, user.session_id, now).await?;
    }
    let mut resp = StatusCode::NO_CONTENT.into_response();
    resp.headers_mut()
        .insert(header::SET_COOKIE, session_cookie("", 0));
    Ok(resp)
}
