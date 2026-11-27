//! `/admin/org` and `/admin/org/domains` (T083, FR-009, FR-010, FR-062).
use crate::app::AppState;
use crate::auth::CurrentUser;
use crate::error::{field_error, ApiError, ApiResult};
use crate::extract::ApiJson;
use crate::org_input;
use axum::extract::{ConnectInfo, Path, State};
use axum::http::StatusCode;
use axum::Json;
use leavo_core::domains::normalize_domain;
use leavo_core::error::ErrorCode;
use leavo_core::permissions::Action;
use leavo_db::audit::{write_audit, AuditEntry};
use leavo_db::orgs_repo::{self, is_unique_violation};
use leavo_db::tx::{begin_org_tx, lock_org};
use rust_decimal::prelude::{FromPrimitive, ToPrimitive};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::net::SocketAddr;
use utoipa::ToSchema;
use uuid::Uuid;

#[derive(Serialize, ToSchema)]
pub struct OrgSettings {
    pub id: Uuid,
    pub name: String,
    pub timezone: String,
    pub weekend: Vec<String>,
    pub leave_year_start_month: i16,
    /// Days of casual leave carried into the next leave year at most.
    pub carry_forward_cap: f64,
    pub comp_off_expiry_days: i32,
}

async fn load(conn: &mut sqlx::PgConnection, org: Uuid) -> ApiResult<OrgSettings> {
    let (name, timezone, mask, start, cap, expiry): (String, String, i16, i16, Decimal, i32) = sqlx::query_as(
        "SELECT name, timezone, weekend_mask, leave_year_start_month, carry_forward_cap, comp_off_expiry_days
         FROM organizations WHERE id = $1",
    )
    .bind(org)
    .fetch_one(conn)
    .await?;
    Ok(OrgSettings {
        id: org,
        name,
        timezone,
        weekend: org_input::weekend_names(mask),
        leave_year_start_month: start,
        carry_forward_cap: cap.to_f64().unwrap_or_default(),
        comp_off_expiry_days: expiry,
    })
}

#[utoipa::path(get, path = "/api/v1/admin/org", tag = "admin",
    responses((status = 200, description = "Organization settings", body = OrgSettings),
              (status = 403, description = "Not an admin", body = crate::openapi::ErrorBody)))]
pub async fn get_org(
    State(state): State<AppState>,
    user: CurrentUser,
) -> ApiResult<Json<OrgSettings>> {
    user.require(Action::ManagePolicy)?;
    let mut tx = begin_org_tx(&state.db, user.org_id).await?;
    let s = load(&mut tx, user.org_id).await?;
    tx.commit().await?;
    Ok(Json(s))
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct PatchOrg {
    pub name: Option<String>,
    pub timezone: Option<String>,
    pub weekend: Option<Vec<String>>,
    /// Locked once leave data exists (`409 LEAVE_YEAR_LOCKED`).
    pub leave_year_start_month: Option<i64>,
    pub carry_forward_cap: Option<f64>,
    pub comp_off_expiry_days: Option<i64>,
}

#[utoipa::path(patch, path = "/api/v1/admin/org", tag = "admin", request_body = PatchOrg,
    responses(
        (status = 200, description = "Updated. Time zone and weekend changes apply to requests submitted from now on (FR-062)", body = OrgSettings),
        (status = 403, description = "Not an admin", body = crate::openapi::ErrorBody),
        (status = 409, description = "LEAVE_YEAR_LOCKED", body = crate::openapi::ErrorBody),
        (status = 422, description = "VALIDATION", body = crate::openapi::ErrorBody),
    ))]
pub async fn patch_org(
    State(state): State<AppState>,
    user: CurrentUser,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    ApiJson(p): ApiJson<PatchOrg>,
) -> ApiResult<Json<OrgSettings>> {
    user.require(Action::ManagePolicy)?;
    let mut tx = begin_org_tx(&state.db, user.org_id).await?;
    lock_org(&mut tx, user.org_id).await?;
    let before = load(&mut tx, user.org_id).await?;

    let name = p
        .name
        .as_deref()
        .map(|n| org_input::name("name", n))
        .transpose()?;
    let timezone = p
        .timezone
        .as_deref()
        .map(|t| org_input::timezone("timezone", t))
        .transpose()?;
    let mask = p
        .weekend
        .as_deref()
        .map(|w| org_input::weekend("weekend", w))
        .transpose()?;
    let start = p
        .leave_year_start_month
        .map(|m| org_input::start_month("leave_year_start_month", m))
        .transpose()?;
    let cap = p
        .carry_forward_cap
        .map(|c| {
            Decimal::from_f64(c)
                .map(|d| d.round_dp(1))
                .filter(|d| *d >= Decimal::ZERO && *d <= Decimal::from(365))
                .ok_or_else(|| field_error("carry_forward_cap"))
        })
        .transpose()?;
    let expiry = p
        .comp_off_expiry_days
        .map(|d| {
            if (1..=365).contains(&d) {
                Ok(d as i32)
            } else {
                Err(field_error("comp_off_expiry_days"))
            }
        })
        .transpose()?;

    if start.is_some_and(|m| m != before.leave_year_start_month) {
        // FR-062: only while there are no leave requests and nothing but initial allotments.
        let locked: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM balance_ledger WHERE reason NOT IN ('allotment', 'prorate'))
                 OR EXISTS (SELECT 1 FROM leave_requests)",
        )
        .fetch_one(&mut *tx)
        .await?;
        if locked {
            return Err(ApiError::new(ErrorCode::LeaveYearLocked));
        }
    }

    sqlx::query(
        "UPDATE organizations SET
             name = COALESCE($2, name), timezone = COALESCE($3, timezone),
             weekend_mask = COALESCE($4, weekend_mask),
             leave_year_start_month = COALESCE($5, leave_year_start_month),
             carry_forward_cap = COALESCE($6, carry_forward_cap),
             comp_off_expiry_days = COALESCE($7, comp_off_expiry_days)
         WHERE id = $1",
    )
    .bind(user.org_id)
    .bind(name)
    .bind(timezone)
    .bind(mask)
    .bind(start)
    .bind(cap)
    .bind(expiry)
    .execute(&mut *tx)
    .await?;
    let after = load(&mut tx, user.org_id).await?;
    write_audit(
        &mut tx,
        &AuditEntry {
            org_id: user.org_id,
            actor_id: Some(user.id),
            action: "org.update",
            entity_kind: "organization",
            entity_id: Some(user.org_id),
            before: Some(serde_json::to_value(&before).map_err(ApiError::internal)?),
            after: Some(serde_json::to_value(&after).map_err(ApiError::internal)?),
            ip: Some(peer.ip()),
        },
    )
    .await?;
    tx.commit().await?;
    Ok(Json(after))
}

#[derive(Serialize, ToSchema)]
pub struct Domain {
    pub id: Uuid,
    pub domain: String,
}

#[utoipa::path(get, path = "/api/v1/admin/org/domains", tag = "admin",
    responses((status = 200, description = "Whitelisted domains", body = [Domain]),
              (status = 403, description = "Not an admin", body = crate::openapi::ErrorBody)))]
pub async fn list_domains(
    State(state): State<AppState>,
    user: CurrentUser,
) -> ApiResult<Json<Vec<Domain>>> {
    user.require(Action::ManagePolicy)?;
    let mut tx = begin_org_tx(&state.db, user.org_id).await?;
    let rows = orgs_repo::list_domains(&mut tx).await?;
    tx.commit().await?;
    Ok(Json(
        rows.into_iter()
            .map(|(id, domain)| Domain { id, domain })
            .collect(),
    ))
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct AddDomain {
    pub domain: String,
}

#[utoipa::path(post, path = "/api/v1/admin/org/domains", tag = "admin", request_body = AddDomain,
    responses(
        (status = 201, description = "Added", body = Domain),
        (status = 403, description = "Not an admin", body = crate::openapi::ErrorBody),
        (status = 409, description = "DOMAIN_TAKEN", body = crate::openapi::ErrorBody),
        (status = 422, description = "INVALID_DOMAIN or PUBLIC_DOMAIN", body = crate::openapi::ErrorBody),
    ))]
pub async fn add_domain(
    State(state): State<AppState>,
    user: CurrentUser,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    ApiJson(b): ApiJson<AddDomain>,
) -> ApiResult<(StatusCode, Json<Domain>)> {
    user.require(Action::ManagePolicy)?;
    let domain = normalize_domain(&b.domain).map_err(ApiError::new)?;
    let mut tx = begin_org_tx(&state.db, user.org_id).await?;
    let id = match orgs_repo::add_domain(&mut tx, user.org_id, &domain).await {
        Ok(id) => id,
        Err(e) if is_unique_violation(&e) => return Err(ApiError::new(ErrorCode::DomainTaken)),
        Err(e) => return Err(e.into()),
    };
    audit_domain(&mut tx, &user, peer, "org.domain_add", id, &domain).await?;
    tx.commit().await?;
    Ok((StatusCode::CREATED, Json(Domain { id, domain })))
}

#[utoipa::path(delete, path = "/api/v1/admin/org/domains/{id}", tag = "admin",
    params(("id" = Uuid, Path, description = "Domain id")),
    responses(
        (status = 204, description = "Removed: no new signups from it; existing users keep access"),
        (status = 403, description = "Not an admin", body = crate::openapi::ErrorBody),
        (status = 404, description = "Not found", body = crate::openapi::ErrorBody),
    ))]
pub async fn remove_domain(
    State(state): State<AppState>,
    user: CurrentUser,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Path(id): Path<Uuid>,
) -> ApiResult<StatusCode> {
    user.require(Action::ManagePolicy)?;
    let mut tx = begin_org_tx(&state.db, user.org_id).await?;
    let domain = orgs_repo::list_domains(&mut tx)
        .await?
        .into_iter()
        .find(|(d, _)| *d == id)
        .map(|(_, name)| name)
        .ok_or_else(|| ApiError::new(ErrorCode::NotFound))?;
    orgs_repo::remove_domain(&mut tx, id).await?;
    audit_domain(&mut tx, &user, peer, "org.domain_remove", id, &domain).await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn audit_domain(
    conn: &mut sqlx::PgConnection,
    user: &CurrentUser,
    peer: SocketAddr,
    action: &str,
    id: Uuid,
    domain: &str,
) -> ApiResult<()> {
    write_audit(
        conn,
        &AuditEntry {
            org_id: user.org_id,
            actor_id: Some(user.id),
            action,
            entity_kind: "org_domain",
            entity_id: Some(id),
            before: None,
            after: Some(json!({ "domain": domain })),
            ip: Some(peer.ip()),
        },
    )
    .await?;
    Ok(())
}

#[utoipa::path(post, path = "/api/v1/admin/org/sign-out-all", tag = "admin",
    responses(
        (status = 204, description = "Every session in the organization ended, including the caller's (FR-063)"),
        (status = 403, description = "Not an admin", body = crate::openapi::ErrorBody),
    ))]
pub async fn sign_out_all(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    user: CurrentUser,
) -> ApiResult<StatusCode> {
    user.require(leavo_core::permissions::Action::ManageUsers)?;
    let now = state.clock.now();
    let mut tx = begin_org_tx(&state.db, user.org_id).await?;
    let ended = leavo_db::sessions::revoke_all_for_org(&mut *tx, user.org_id, now).await?;
    write_audit(
        &mut tx,
        &AuditEntry {
            org_id: user.org_id,
            actor_id: Some(user.id),
            action: "org.sign_out_all",
            entity_kind: "organization",
            entity_id: Some(user.org_id),
            before: None,
            after: Some(serde_json::json!({ "sessions_ended": ended })),
            ip: Some(peer.ip()),
        },
    )
    .await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}
