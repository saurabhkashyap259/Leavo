//! `/setup/*` — the first-run wizard (T081, FR-007).
use crate::app::AppState;
use crate::bootstrap::{self, NewOrgWithAdmin};
use crate::config::InstanceMode;
use crate::error::{field_error, ApiError, ApiResult};
use crate::extract::ApiJson;
use crate::holiday_packs::{self, PackInfo};
use crate::org_input;
use crate::routes::auth::{
    check_code, start_session, with_session_cookie, CodePurpose, SignedIn, SignedInUser,
};
use crate::setup::token;
use axum::extract::{ConnectInfo, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use leavo_core::domains::normalize_domain;
use leavo_core::error::ErrorCode;
use leavo_core::otp;
use leavo_email::EmailMessage;
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use utoipa::ToSchema;

#[derive(Serialize, ToSchema)]
pub struct SetupStatus {
    pub setup_required: bool,
    /// `single` or `multi`
    pub mode: &'static str,
    pub holiday_packs: Vec<PackInfo>,
}

#[utoipa::path(get, path = "/api/v1/setup/status", tag = "setup",
    responses((status = 200, description = "Whether the first-run wizard is needed", body = SetupStatus)))]
pub async fn status(State(state): State<AppState>) -> ApiResult<Json<SetupStatus>> {
    Ok(Json(SetupStatus {
        setup_required: token::setup_required(&state.db).await?,
        mode: match state.config.mode {
            InstanceMode::Single => "single",
            InstanceMode::Multi => "multi",
        },
        holiday_packs: holiday_packs::list(),
    }))
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct TokenBody {
    pub setup_token: String,
}

#[utoipa::path(post, path = "/api/v1/setup/verify-token", tag = "setup", request_body = TokenBody,
    responses(
        (status = 204, description = "Token is valid"),
        (status = 403, description = "SETUP_TOKEN_INVALID", body = crate::openapi::ErrorBody),
        (status = 429, description = "LOCKED after 5 wrong tries (Retry-After header)", body = crate::openapi::ErrorBody),
    ))]
pub async fn verify_token(
    State(state): State<AppState>,
    ApiJson(b): ApiJson<TokenBody>,
) -> ApiResult<StatusCode> {
    token::check(&state.db, &b.setup_token, state.clock.now()).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct TestEmailBody {
    pub setup_token: String,
    pub to: String,
}

#[derive(Serialize, ToSchema)]
pub struct TestEmailResult {
    pub ok: bool,
    /// The SMTP server's error text when sending failed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[utoipa::path(post, path = "/api/v1/setup/test-email", tag = "setup", request_body = TestEmailBody,
    responses(
        (status = 200, description = "Sent now (not queued); `ok: false` carries the SMTP error", body = TestEmailResult),
        (status = 403, description = "SETUP_TOKEN_INVALID", body = crate::openapi::ErrorBody),
    ))]
pub async fn test_email(
    State(state): State<AppState>,
    ApiJson(b): ApiJson<TestEmailBody>,
) -> ApiResult<Json<TestEmailResult>> {
    token::check(&state.db, &b.setup_token, state.clock.now()).await?;
    let to = otp::normalize_email(&b.to).ok_or_else(|| field_error("to"))?;
    let ctx = serde_json::json!({ "org_name": "Leavo", "base_url": state.config.base_url });
    let rendered = state
        .templates
        .render("test_email", &ctx)
        .map_err(ApiError::internal)?;
    let msg = EmailMessage {
        to,
        subject: rendered.subject,
        html: rendered.html,
        text: rendered.text,
        attachments: vec![],
    };
    Ok(Json(match state.email.send(&msg).await {
        Ok(()) => TestEmailResult {
            ok: true,
            error: None,
        },
        Err(e) => TestEmailResult {
            ok: false,
            error: Some(e.to_string()),
        },
    }))
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct SetupOrg {
    pub name: String,
    pub timezone: String,
    /// e.g. `["sat","sun"]`
    pub weekend: Vec<String>,
    pub leave_year_start_month: i64,
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct SetupAdmin {
    pub full_name: String,
    pub email: String,
    /// The 6-digit code sent by `POST /auth/code` with purpose `setup`.
    pub code: String,
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CompleteSetup {
    pub setup_token: String,
    pub org: SetupOrg,
    pub admin: SetupAdmin,
    /// Whitelisted besides the admin's own domain.
    #[serde(default)]
    pub extra_domains: Vec<String>,
    /// e.g. `IN/2026`; see `/setup/status`.
    pub holiday_pack: Option<String>,
    #[serde(default = "yes")]
    pub seed_leave_types: bool,
}

fn yes() -> bool {
    true
}

#[utoipa::path(post, path = "/api/v1/setup/complete", tag = "setup", request_body = CompleteSetup,
    responses(
        (status = 200, description = "Organization created; the admin is signed in (sets leavo_sid)", body = SignedIn),
        (status = 403, description = "SETUP_TOKEN_INVALID (also once setup is complete)", body = crate::openapi::ErrorBody),
        (status = 422, description = "VALIDATION, PUBLIC_DOMAIN, INVALID_DOMAIN or CODE_INVALID", body = crate::openapi::ErrorBody),
        (status = 429, description = "LOCKED", body = crate::openapi::ErrorBody),
    ))]
pub async fn complete(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    ApiJson(b): ApiJson<CompleteSetup>,
) -> ApiResult<Response> {
    let now = state.clock.now();
    token::check(&state.db, &b.setup_token, now).await?;

    let name = org_input::name("org.name", &b.org.name)?;
    let timezone = org_input::timezone("org.timezone", &b.org.timezone)?;
    let weekend_mask = org_input::weekend("org.weekend", &b.org.weekend)?;
    let start = org_input::start_month("org.leave_year_start_month", b.org.leave_year_start_month)?;
    let admin_name = org_input::name("admin.full_name", &b.admin.full_name)?;
    let email = otp::normalize_email(&b.admin.email).ok_or_else(|| field_error("admin.email"))?;
    let mut domains =
        vec![normalize_domain(otp::domain_of(&email).unwrap_or_default()).map_err(ApiError::new)?];
    for d in &b.extra_domains {
        let d = normalize_domain(d).map_err(ApiError::new)?;
        if !domains.contains(&d) {
            domains.push(d);
        }
    }
    if let Some(p) = &b.holiday_pack {
        if holiday_packs::get(p).is_none() {
            return Err(field_error("holiday_pack"));
        }
    }

    check_code(&state, &email, CodePurpose::Setup, &b.admin.code).await?;

    let mut tx = state.db.begin().await?;
    // Serialise concurrent completions; only the first one wins.
    sqlx::query("SELECT 1 FROM instance_settings WHERE id = 1 FOR UPDATE")
        .execute(&mut *tx)
        .await?;
    let still_open: bool = sqlx::query_scalar(
        "SELECT setup_token_hash IS NOT NULL AND NOT instance_has_org() FROM instance_settings WHERE id = 1",
    )
    .fetch_one(&mut *tx)
    .await?;
    if !still_open {
        return Err(ApiError::new(ErrorCode::SetupTokenInvalid));
    }
    let (org, user) = bootstrap::create(
        &mut tx,
        &NewOrgWithAdmin {
            name: &name,
            timezone: &timezone,
            weekend_mask,
            leave_year_start_month: start,
            admin_email: &email,
            admin_name: &admin_name,
            domains: &domains,
            holiday_pack: b.holiday_pack.as_deref(),
            seed_leave_types: b.seed_leave_types,
            audit_action: "org.setup",
            ip: Some(peer.ip()),
        },
        now,
    )
    .await?;
    token::retire(&mut tx, now).await?;
    let sid = start_session(&mut tx, user, org, false, &headers, peer.ip(), now).await?;
    tx.commit().await?;
    tracing::info!(org_id = %org, "first-run setup completed");

    let body = SignedIn {
        user: SignedInUser {
            id: user,
            org_id: org,
            email,
            full_name: admin_name,
            role: "admin".into(),
        },
    };
    Ok(with_session_cookie(Json(body).into_response(), &sid))
}
