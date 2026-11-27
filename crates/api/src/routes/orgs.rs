//! `POST /orgs` — public organization registration, multi-org mode only (T082, FR-008).
use crate::app::AppState;
use crate::bootstrap::{self, NewOrgWithAdmin};
use crate::config::InstanceMode;
use crate::error::{ApiError, ApiResult};
use crate::extract::ApiJson;
use crate::org_input;
use crate::routes::auth::{start_session, with_session_cookie, CodePurpose, SignedInUser};
use crate::routes::me::OrgSummary;
use axum::extract::{ConnectInfo, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use leavo_core::domains::normalize_domain;
use leavo_core::error::ErrorCode;
use leavo_core::otp;
use leavo_core::tokens::verify_ticket;
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use utoipa::ToSchema;

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct RegisterOrg {
    /// From `POST /auth/verify` with purpose `register_org`.
    pub ticket: String,
    pub name: String,
    pub timezone: String,
    pub leave_year_start_month: i64,
    pub full_name: String,
    /// Defaults to `["sat","sun"]`.
    pub weekend: Option<Vec<String>>,
}

#[derive(Serialize, ToSchema)]
pub struct RegisteredOrg {
    pub org: OrgSummary,
    pub user: SignedInUser,
}

#[utoipa::path(post, path = "/api/v1/orgs", tag = "setup", request_body = RegisterOrg,
    responses(
        (status = 201, description = "Organization created with default leave types; the founder is its admin and signed in", body = RegisteredOrg),
        (status = 404, description = "Single-organization mode", body = crate::openapi::ErrorBody),
        (status = 409, description = "DOMAIN_TAKEN", body = crate::openapi::ErrorBody),
        (status = 422, description = "VALIDATION or CODE_INVALID (bad or expired ticket)", body = crate::openapi::ErrorBody),
    ))]
pub async fn register(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    ApiJson(b): ApiJson<RegisterOrg>,
) -> ApiResult<Response> {
    if state.config.mode != InstanceMode::Multi {
        return Err(ApiError::new(ErrorCode::NotFound));
    }
    let now = state.clock.now();
    let email = verify_ticket(
        state.config.secret_key.as_bytes(),
        &b.ticket,
        CodePurpose::RegisterOrg.as_str(),
        now.timestamp(),
    )
    .ok_or_else(|| ApiError::new(ErrorCode::CodeInvalid))?;
    let name = org_input::name("name", &b.name)?;
    let timezone = org_input::timezone("timezone", &b.timezone)?;
    let start = org_input::start_month("leave_year_start_month", b.leave_year_start_month)?;
    let weekend = b
        .weekend
        .unwrap_or_else(|| vec!["sat".into(), "sun".into()]);
    let weekend_mask = org_input::weekend("weekend", &weekend)?;
    let full_name = org_input::name("full_name", &b.full_name)?;
    let domain =
        normalize_domain(otp::domain_of(&email).unwrap_or_default()).map_err(ApiError::new)?;

    let mut tx = state.db.begin().await?;
    let (org, user) = bootstrap::create(
        &mut tx,
        &NewOrgWithAdmin {
            name: &name,
            timezone: &timezone,
            weekend_mask,
            leave_year_start_month: start,
            admin_email: &email,
            admin_name: &full_name,
            domains: std::slice::from_ref(&domain),
            holiday_pack: None,
            seed_leave_types: true,
            audit_action: "org.register",
            ip: Some(peer.ip()),
        },
        now,
    )
    .await?;
    let sid = start_session(&mut tx, user, org, false, &headers, peer.ip(), now).await?;
    tx.commit().await?;

    let body = RegisteredOrg {
        org: OrgSummary {
            id: org,
            name,
            timezone,
            weekend: org_input::weekend_names(weekend_mask),
            leave_year_start_month: start,
        },
        user: SignedInUser {
            id: user,
            org_id: org,
            email,
            full_name,
            role: "admin".into(),
        },
    };
    Ok(with_session_cookie(
        (StatusCode::CREATED, Json(body)).into_response(),
        &sid,
    ))
}
