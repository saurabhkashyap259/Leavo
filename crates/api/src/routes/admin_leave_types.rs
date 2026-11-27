//! `/admin/leave-types` and restricted-type grants (T084, FR-011, FR-012, FR-056).
use crate::app::AppState;
use crate::auth::CurrentUser;
use crate::error::{field_error, ApiError, ApiResult};
use crate::extract::ApiJson;
use axum::extract::{ConnectInfo, Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use chrono::{DateTime, Utc};
use leavo_core::calendar::{leave_year_bounds, leave_year_of};
use leavo_core::error::ErrorCode;
use leavo_core::permissions::Action;
use leavo_core::policy::quota_change;
use leavo_db::audit::{write_audit, AuditEntry};
use leavo_db::leave_types_repo::{self as repo, GrantRow, LeaveTypeRow};
use leavo_db::ledger::{insert_entry, LedgerEntry, LedgerReason};
use leavo_db::tx::{begin_org_tx, lock_balance};
use leavo_db::users_repo::org_today;
use rust_decimal::prelude::{FromPrimitive, ToPrimitive};
use rust_decimal::Decimal;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::json;
use sqlx::PgConnection;
use std::net::SocketAddr;
use utoipa::ToSchema;
use uuid::Uuid;

fn num(d: Decimal) -> f64 {
    d.to_f64().unwrap_or_default()
}

#[derive(Serialize, ToSchema)]
pub struct LeaveType {
    pub id: Uuid,
    pub code: String,
    pub name: String,
    pub color: String,
    /// `null` for the comp-off type (earned, not allotted).
    pub yearly_quota: Option<f64>,
    /// Quota that takes effect at the next leave year (FR-056).
    pub pending_yearly_quota: Option<f64>,
    pub is_comp_off: bool,
    pub receives_carry_forward: bool,
    pub allow_half_day: bool,
    pub allow_negative: bool,
    pub negative_limit: f64,
    pub max_days_per_request: Option<f64>,
    pub min_notice_days: i32,
    pub max_backdate_days: i32,
    pub attachment_required_above_days: Option<f64>,
    /// `all` or `restricted`
    pub availability: String,
    pub active: bool,
    pub sort_order: i32,
    pub granted_count: i64,
}

impl From<LeaveTypeRow> for LeaveType {
    fn from(t: LeaveTypeRow) -> Self {
        Self {
            id: t.id,
            code: t.code,
            name: t.name,
            color: t.color,
            yearly_quota: t.yearly_quota.map(num),
            pending_yearly_quota: t.pending_yearly_quota.map(num),
            is_comp_off: t.is_comp_off,
            receives_carry_forward: t.receives_carry_forward,
            allow_half_day: t.allow_half_day,
            allow_negative: t.allow_negative,
            negative_limit: num(t.negative_limit),
            max_days_per_request: t.max_days_per_request.map(num),
            min_notice_days: t.min_notice_days,
            max_backdate_days: t.max_backdate_days,
            attachment_required_above_days: t.attachment_required_above_days.map(num),
            availability: t.availability,
            active: t.active,
            sort_order: t.sort_order,
            granted_count: t.granted_count,
        }
    }
}

/// Distinguish "absent" (outer `None`) from explicit `null` (`Some(None)`).
fn nullable<'de, D: Deserializer<'de>, T: Deserialize<'de>>(
    d: D,
) -> Result<Option<Option<T>>, D::Error> {
    Option::<T>::deserialize(d).map(Some)
}

/// Days: ≥ 0, ≤ `max`, whole or half.
fn days(field: &str, v: f64, max: i64) -> ApiResult<Decimal> {
    let d = Decimal::from_f64(v)
        .ok_or_else(|| field_error(field))?
        .round_dp(1);
    let half = d * Decimal::from(2);
    if d < Decimal::ZERO || d > Decimal::from(max) || half != half.trunc() {
        return Err(field_error(field));
    }
    Ok(d)
}

fn valid_code(c: &str) -> bool {
    (1..=6).contains(&c.len()) && c.bytes().all(|b| b.is_ascii_uppercase())
}

fn valid_color(c: &str) -> bool {
    c.len() == 7 && c.starts_with('#') && c[1..].bytes().all(|b| b.is_ascii_hexdigit())
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct LeaveTypeInput {
    pub code: Option<String>,
    pub name: Option<String>,
    pub color: Option<String>,
    #[serde(default, deserialize_with = "nullable")]
    #[schema(value_type = Option<f64>)]
    pub yearly_quota: Option<Option<f64>>,
    /// Required when `yearly_quota` changes: `now` or `next_year` (FR-056).
    pub apply_from: Option<ApplyFrom>,
    pub receives_carry_forward: Option<bool>,
    pub allow_half_day: Option<bool>,
    pub allow_negative: Option<bool>,
    pub negative_limit: Option<f64>,
    #[serde(default, deserialize_with = "nullable")]
    #[schema(value_type = Option<f64>)]
    pub max_days_per_request: Option<Option<f64>>,
    pub min_notice_days: Option<i64>,
    pub max_backdate_days: Option<i64>,
    #[serde(default, deserialize_with = "nullable")]
    #[schema(value_type = Option<f64>)]
    pub attachment_required_above_days: Option<Option<f64>>,
    pub availability: Option<String>,
    pub active: Option<bool>,
    pub sort_order: Option<i32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ApplyFrom {
    Now,
    NextYear,
}

/// Apply every field except the quota onto `t`.
async fn apply_fields(
    conn: &mut PgConnection,
    t: &mut LeaveTypeRow,
    i: &LeaveTypeInput,
) -> ApiResult<()> {
    if let Some(code) = &i.code {
        let code = code.trim().to_string();
        if !valid_code(&code) || repo::code_taken(conn, &code, Some(t.id)).await? {
            return Err(field_error("code"));
        }
        t.code = code;
    }
    if let Some(name) = &i.name {
        let name = name.trim();
        if name.is_empty() || name.chars().count() > 60 {
            return Err(field_error("name"));
        }
        t.name = name.to_string();
    }
    if let Some(color) = &i.color {
        if !valid_color(color) {
            return Err(field_error("color"));
        }
        t.color = color.to_uppercase();
    }
    if let Some(v) = i.receives_carry_forward {
        t.receives_carry_forward = v;
    }
    if let Some(v) = i.allow_half_day {
        t.allow_half_day = v;
    }
    if let Some(v) = i.allow_negative {
        t.allow_negative = v;
    }
    if let Some(v) = i.negative_limit {
        t.negative_limit = days("negative_limit", v, 365)?;
    }
    if let Some(v) = i.max_days_per_request {
        t.max_days_per_request = v
            .map(|d| {
                days("max_days_per_request", d, 365).and_then(|d| {
                    if d.is_zero() {
                        Err(field_error("max_days_per_request"))
                    } else {
                        Ok(d)
                    }
                })
            })
            .transpose()?;
    }
    if let Some(v) = i.min_notice_days {
        if !(0..=365).contains(&v) {
            return Err(field_error("min_notice_days"));
        }
        t.min_notice_days = v as i32;
    }
    if let Some(v) = i.max_backdate_days {
        if !(0..=30).contains(&v) {
            return Err(field_error("max_backdate_days"));
        }
        t.max_backdate_days = v as i32;
    }
    if let Some(v) = i.attachment_required_above_days {
        t.attachment_required_above_days = v
            .map(|d| days("attachment_required_above_days", d, 365))
            .transpose()?;
    }
    if let Some(a) = &i.availability {
        if a != "all" && a != "restricted" {
            return Err(field_error("availability"));
        }
        t.availability = a.clone();
    }
    if let Some(v) = i.active {
        t.active = v;
    }
    if let Some(v) = i.sort_order {
        t.sort_order = v;
    }
    Ok(())
}

#[utoipa::path(get, path = "/api/v1/admin/leave-types", tag = "admin",
    responses((status = 200, description = "Every leave type, including inactive ones", body = [LeaveType]),
              (status = 403, description = "Not an admin", body = crate::openapi::ErrorBody)))]
pub async fn list(
    State(state): State<AppState>,
    user: CurrentUser,
) -> ApiResult<Json<Vec<LeaveType>>> {
    user.require(Action::ManagePolicy)?;
    let mut tx = begin_org_tx(&state.db, user.org_id).await?;
    let rows = repo::list(&mut tx).await?;
    tx.commit().await?;
    Ok(Json(rows.into_iter().map(LeaveType::from).collect()))
}

#[utoipa::path(post, path = "/api/v1/admin/leave-types", tag = "admin", request_body = LeaveTypeInput,
    responses(
        (status = 201, description = "Created. `code`, `name`, `color` and `yearly_quota` are required", body = LeaveType),
        (status = 403, description = "Not an admin", body = crate::openapi::ErrorBody),
        (status = 422, description = "VALIDATION (details.field)", body = crate::openapi::ErrorBody),
    ))]
pub async fn create(
    State(state): State<AppState>,
    user: CurrentUser,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    ApiJson(i): ApiJson<LeaveTypeInput>,
) -> ApiResult<(StatusCode, Json<LeaveType>)> {
    user.require(Action::ManagePolicy)?;
    for (field, present) in [
        ("code", i.code.is_some()),
        ("name", i.name.is_some()),
        ("color", i.color.is_some()),
    ] {
        if !present {
            return Err(field_error(field));
        }
    }
    let quota = match i.yearly_quota {
        Some(Some(q)) => days("yearly_quota", q, 999)?,
        _ => return Err(field_error("yearly_quota")),
    };
    let mut tx = begin_org_tx(&state.db, user.org_id).await?;
    let mut t = LeaveTypeRow {
        id: Uuid::now_v7(),
        code: String::new(),
        name: String::new(),
        color: String::new(),
        yearly_quota: Some(quota),
        pending_yearly_quota: None,
        is_comp_off: false,
        receives_carry_forward: false,
        allow_half_day: true,
        allow_negative: false,
        negative_limit: Decimal::from(5),
        max_days_per_request: None,
        min_notice_days: 0,
        max_backdate_days: 0,
        attachment_required_above_days: None,
        availability: "all".into(),
        active: true,
        sort_order: repo::next_sort_order(&mut tx).await?,
        granted_count: 0,
    };
    apply_fields(&mut tx, &mut t, &i).await?;
    repo::upsert(&mut tx, user.org_id, &t).await?;
    audit(&mut tx, &user, peer, "policy.create", t.id, None, Some(&t)).await?;
    tx.commit().await?;
    Ok((StatusCode::CREATED, Json(t.into())))
}

#[utoipa::path(patch, path = "/api/v1/admin/leave-types/{id}", tag = "admin", request_body = LeaveTypeInput,
    params(("id" = Uuid, Path, description = "Leave type id")),
    responses(
        (status = 200, description = "Updated", body = LeaveType),
        (status = 403, description = "Not an admin", body = crate::openapi::ErrorBody),
        (status = 404, description = "Not found", body = crate::openapi::ErrorBody),
        (status = 422, description = "VALIDATION or QUOTA_APPLY_FROM_REQUIRED", body = crate::openapi::ErrorBody),
    ))]
pub async fn patch(
    State(state): State<AppState>,
    user: CurrentUser,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Path(id): Path<Uuid>,
    ApiJson(i): ApiJson<LeaveTypeInput>,
) -> ApiResult<Json<LeaveType>> {
    user.require(Action::ManagePolicy)?;
    let now = state.clock.now();
    let mut tx = begin_org_tx(&state.db, user.org_id).await?;
    let before = repo::get_for_update(&mut tx, id)
        .await?
        .ok_or_else(|| ApiError::new(ErrorCode::NotFound))?;
    let mut t = before.clone();
    apply_fields(&mut tx, &mut t, &i).await?;

    let mut quota_rows = 0;
    if let Some(q) = i.yearly_quota {
        let new = match (q, t.is_comp_off) {
            (Some(q), false) => days("yearly_quota", q, 999)?,
            _ => return Err(field_error("yearly_quota")),
        };
        let old = before.yearly_quota.unwrap_or_default();
        if new != old {
            match i.apply_from {
                None => return Err(ApiError::new(ErrorCode::QuotaApplyFromRequired)),
                Some(ApplyFrom::NextYear) => t.pending_yearly_quota = Some(new),
                Some(ApplyFrom::Now) => {
                    quota_rows = apply_quota_now(&mut tx, &user, &before, old, new, now).await?;
                    t.yearly_quota = Some(new);
                    t.pending_yearly_quota = None;
                }
            }
        }
    }
    repo::upsert(&mut tx, user.org_id, &t).await?;
    let after = repo::get(&mut tx, id)
        .await?
        .ok_or_else(|| ApiError::new(ErrorCode::NotFound))?;
    audit(
        &mut tx,
        &user,
        peer,
        "policy.update",
        id,
        Some(&before),
        Some(&after),
    )
    .await?;
    if quota_rows > 0 {
        tracing::info!(leave_type = %id, rows = quota_rows, "quota change applied");
    }
    tx.commit().await?;
    Ok(Json(after.into()))
}

/// FR-056 "apply from now": one `quota_change` row per affected user in the current leave year.
async fn apply_quota_now(
    conn: &mut PgConnection,
    user: &CurrentUser,
    t: &LeaveTypeRow,
    old: Decimal,
    new: Decimal,
    now: DateTime<Utc>,
) -> ApiResult<usize> {
    let (today, start_month) = org_today(conn, user.org_id, now).await?;
    let leave_year = leave_year_of(today, start_month);
    let (year_start, _) = leave_year_bounds(leave_year, start_month);
    let delta = quota_change(old, new, today, year_start, start_month);
    if delta.is_zero() {
        return Ok(0);
    }
    let users = repo::affected_users(conn, t).await?;
    for u in &users {
        lock_balance(conn, *u, t.id).await?;
        insert_entry(
            conn,
            &LedgerEntry {
                org_id: user.org_id,
                user_id: *u,
                leave_type_id: t.id,
                leave_year,
                delta,
                reason: LedgerReason::QuotaChange,
                ref_kind: Some("leave_type".into()),
                ref_id: Some(t.id),
                note: Some(format!("Quota {old} → {new}")),
                created_by: Some(user.id),
            },
        )
        .await?;
    }
    Ok(users.len())
}

fn snapshot(t: &LeaveTypeRow) -> serde_json::Value {
    json!({
        "code": t.code, "name": t.name, "color": t.color, "yearly_quota": t.yearly_quota,
        "pending_yearly_quota": t.pending_yearly_quota, "allow_half_day": t.allow_half_day,
        "allow_negative": t.allow_negative, "negative_limit": t.negative_limit,
        "max_days_per_request": t.max_days_per_request, "min_notice_days": t.min_notice_days,
        "max_backdate_days": t.max_backdate_days,
        "attachment_required_above_days": t.attachment_required_above_days,
        "availability": t.availability, "active": t.active,
        "receives_carry_forward": t.receives_carry_forward,
    })
}

#[allow(clippy::too_many_arguments)]
async fn audit(
    conn: &mut PgConnection,
    user: &CurrentUser,
    peer: SocketAddr,
    action: &str,
    id: Uuid,
    before: Option<&LeaveTypeRow>,
    after: Option<&LeaveTypeRow>,
) -> ApiResult<()> {
    write_audit(
        conn,
        &AuditEntry {
            org_id: user.org_id,
            actor_id: Some(user.id),
            action,
            entity_kind: "leave_type",
            entity_id: Some(id),
            before: before.map(snapshot),
            after: after.map(snapshot),
            ip: Some(peer.ip()),
        },
    )
    .await?;
    Ok(())
}

// ---- grants -------------------------------------------------------------------------------

#[derive(Serialize, ToSchema)]
pub struct Grant {
    pub id: Uuid,
    pub user_id: Uuid,
    pub email: String,
    pub full_name: String,
    pub granted_by: Option<Uuid>,
    pub granted_at: DateTime<Utc>,
    pub revoked_by: Option<Uuid>,
    pub revoked_at: Option<DateTime<Utc>>,
}

impl From<GrantRow> for Grant {
    fn from(g: GrantRow) -> Self {
        Self {
            id: g.id,
            user_id: g.user_id,
            email: g.email,
            full_name: g.full_name,
            granted_by: g.granted_by,
            granted_at: g.granted_at,
            revoked_by: g.revoked_by,
            revoked_at: g.revoked_at,
        }
    }
}

#[derive(Deserialize)]
pub struct GrantsQuery {
    #[serde(default)]
    pub include_revoked: bool,
}

#[utoipa::path(get, path = "/api/v1/admin/leave-types/{id}/grants", tag = "admin",
    params(("id" = Uuid, Path, description = "Leave type id"),
           ("include_revoked" = Option<bool>, Query, description = "Include revoked grants (history)")),
    responses((status = 200, description = "Grants, newest first", body = [Grant]),
              (status = 403, description = "Not an admin", body = crate::openapi::ErrorBody)))]
pub async fn list_grants(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<Uuid>,
    Query(q): Query<GrantsQuery>,
) -> ApiResult<Json<Vec<Grant>>> {
    user.require(Action::ManagePolicy)?;
    let mut tx = begin_org_tx(&state.db, user.org_id).await?;
    repo::get(&mut tx, id)
        .await?
        .ok_or_else(|| ApiError::new(ErrorCode::NotFound))?;
    let rows = repo::list_grants(&mut tx, id, q.include_revoked).await?;
    tx.commit().await?;
    Ok(Json(rows.into_iter().map(Grant::from).collect()))
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct GrantInput {
    pub user_id: Option<Uuid>,
    /// Alternative to `user_id`.
    pub email: Option<String>,
}

#[utoipa::path(post, path = "/api/v1/admin/leave-types/{id}/grants", tag = "admin", request_body = GrantInput,
    params(("id" = Uuid, Path, description = "Restricted leave type id")),
    responses(
        (status = 201, description = "Granted; the full yearly quota is allotted for the current leave year (once per year)", body = Grant),
        (status = 200, description = "Already granted", body = Grant),
        (status = 403, description = "Not an admin", body = crate::openapi::ErrorBody),
        (status = 404, description = "Type or user not found", body = crate::openapi::ErrorBody),
        (status = 422, description = "VALIDATION (type is not restricted)", body = crate::openapi::ErrorBody),
    ))]
pub async fn grant(
    State(state): State<AppState>,
    user: CurrentUser,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Path(id): Path<Uuid>,
    ApiJson(b): ApiJson<GrantInput>,
) -> ApiResult<Response> {
    user.require(Action::ManagePolicy)?;
    let now = state.clock.now();
    let mut tx = begin_org_tx(&state.db, user.org_id).await?;
    let t = repo::get_for_update(&mut tx, id)
        .await?
        .ok_or_else(|| ApiError::new(ErrorCode::NotFound))?;
    if t.availability != "restricted" {
        return Err(field_error("leave_type"));
    }
    let grantee: Option<Uuid> = sqlx::query_scalar(
        "SELECT id FROM users WHERE (id = $1 OR email = $2::citext) AND status <> 'deactivated'",
    )
    .bind(b.user_id)
    .bind(b.email.as_deref().map(str::trim))
    .fetch_optional(&mut *tx)
    .await?;
    let grantee = grantee.ok_or_else(|| ApiError::new(ErrorCode::NotFound))?;
    if let Some(existing) = repo::active_grant(&mut tx, id, grantee).await? {
        tx.commit().await?;
        return Ok((StatusCode::OK, Json(Grant::from(existing))).into_response());
    }
    let grant_id = repo::insert_grant(&mut tx, user.org_id, id, grantee, user.id, now).await?;
    let (today, start_month) = org_today(&mut tx, user.org_id, now).await?;
    let leave_year = leave_year_of(today, start_month);
    let quota = t.yearly_quota.unwrap_or_default();
    lock_balance(&mut tx, grantee, id).await?;
    if quota > Decimal::ZERO && !repo::grant_allotted(&mut tx, grantee, id, leave_year).await? {
        insert_entry(
            &mut tx,
            &LedgerEntry {
                org_id: user.org_id,
                user_id: grantee,
                leave_type_id: id,
                leave_year,
                delta: quota,
                reason: LedgerReason::Allotment,
                ref_kind: Some("grant".into()),
                ref_id: Some(grant_id),
                note: None,
                created_by: Some(user.id),
            },
        )
        .await?;
    }
    write_grant_audit(
        &mut tx,
        &user,
        peer,
        "grant.add",
        grant_id,
        grantee,
        &t.code,
    )
    .await?;
    let row = repo::active_grant(&mut tx, id, grantee)
        .await?
        .ok_or_else(|| ApiError::new(ErrorCode::Internal))?;
    tx.commit().await?;
    Ok((StatusCode::CREATED, Json(Grant::from(row))).into_response())
}

#[utoipa::path(delete, path = "/api/v1/admin/leave-types/{id}/grants/{user_id}", tag = "admin",
    params(("id" = Uuid, Path, description = "Leave type id"), ("user_id" = Uuid, Path, description = "Grantee")),
    responses(
        (status = 204, description = "Revoked: remaining available balance lapses; history and pending requests are kept"),
        (status = 403, description = "Not an admin", body = crate::openapi::ErrorBody),
        (status = 404, description = "No active grant", body = crate::openapi::ErrorBody),
    ))]
pub async fn revoke(
    State(state): State<AppState>,
    user: CurrentUser,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Path((id, grantee)): Path<(Uuid, Uuid)>,
) -> ApiResult<StatusCode> {
    user.require(Action::ManagePolicy)?;
    let now = state.clock.now();
    let mut tx = begin_org_tx(&state.db, user.org_id).await?;
    let t = repo::get_for_update(&mut tx, id)
        .await?
        .ok_or_else(|| ApiError::new(ErrorCode::NotFound))?;
    let grant_id = repo::revoke_grant(&mut tx, id, grantee, user.id, now)
        .await?
        .ok_or_else(|| ApiError::new(ErrorCode::NotFound))?;
    let (today, start_month) = org_today(&mut tx, user.org_id, now).await?;
    let leave_year = leave_year_of(today, start_month);
    lock_balance(&mut tx, grantee, id).await?;
    // Lapse what is left after pending requests (`v_balances.available`): pending requests
    // can still be decided after the grant is revoked.
    let balance = leavo_db::leaves_repo::balance_row(&mut tx, grantee, id, Some(leave_year))
        .await?
        .available;
    if balance > Decimal::ZERO {
        insert_entry(
            &mut tx,
            &LedgerEntry {
                org_id: user.org_id,
                user_id: grantee,
                leave_type_id: id,
                leave_year,
                delta: -balance,
                reason: LedgerReason::Lapse,
                ref_kind: Some("grant".into()),
                ref_id: Some(grant_id),
                note: Some("Restricted leave grant revoked".into()),
                created_by: Some(user.id),
            },
        )
        .await?;
    }
    write_grant_audit(
        &mut tx,
        &user,
        peer,
        "grant.revoke",
        grant_id,
        grantee,
        &t.code,
    )
    .await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn write_grant_audit(
    conn: &mut PgConnection,
    user: &CurrentUser,
    peer: SocketAddr,
    action: &str,
    grant: Uuid,
    grantee: Uuid,
    code: &str,
) -> ApiResult<()> {
    write_audit(
        conn,
        &AuditEntry {
            org_id: user.org_id,
            actor_id: Some(user.id),
            action,
            entity_kind: "leave_type_grant",
            entity_id: Some(grant),
            before: None,
            after: Some(json!({ "user_id": grantee, "leave_type": code })),
            ip: Some(peer.ip()),
        },
    )
    .await?;
    Ok(())
}

/// One employee's carry forward for a carry-forward leave type (FR-039).
#[derive(Serialize, ToSchema)]
pub struct CarryForwardRow {
    pub user: CarryForwardUser,
    pub leave_type: crate::routes::leaves::TypeRef,
    /// Balance of the closing leave year before year end (negative for a deficit).
    pub unused: f64,
    /// Carried into the next year (negative: a deficit carried as a deduction).
    pub carry: f64,
    /// Positive days lost.
    pub lapse: f64,
}

#[derive(Serialize, ToSchema)]
pub struct CarryForwardUser {
    pub id: Uuid,
    pub full_name: String,
    pub email: String,
}

#[derive(Deserialize)]
pub struct CarryForwardQuery {
    pub year: Option<String>,
}

async fn carry_forward_rows(
    state: &AppState,
    user: &CurrentUser,
    q: &CarryForwardQuery,
) -> ApiResult<(i32, Vec<leavo_db::rollover_repo::ReportRow>)> {
    user.require(Action::ManagePolicy)?;
    let mut tx = begin_org_tx(&state.db, user.org_id).await?;
    let (today, start_month) = org_today(&mut tx, user.org_id, state.clock.now()).await?;
    let year = match q.year.as_deref() {
        None => leave_year_of(today, start_month),
        Some(y) => y
            .parse::<i32>()
            .ok()
            .filter(|y| (2000..=2200).contains(y))
            .ok_or_else(|| field_error("year"))?,
    };
    let rows = leavo_db::rollover_repo::report(&mut tx, user.org_id, year).await?;
    tx.commit().await?;
    Ok((year, rows))
}

#[utoipa::path(get, path = "/api/v1/admin/carry-forward/preview", tag = "admin",
    params(("year" = Option<i32>, Query, description = "The leave year being closed; defaults to the current one")),
    responses(
        (status = 200, description = "Per active employee and carry-forward type with a non-zero balance: what year end will write (or wrote, once it has run) — the same numbers either way (FR-039)", body = [CarryForwardRow]),
        (status = 403, description = "Not an admin", body = crate::openapi::ErrorBody),
        (status = 422, description = "VALIDATION (details.field: year)", body = crate::openapi::ErrorBody),
    ))]
pub async fn carry_forward_preview(
    State(state): State<AppState>,
    user: CurrentUser,
    Query(q): Query<CarryForwardQuery>,
) -> ApiResult<Json<Vec<CarryForwardRow>>> {
    let (_, rows) = carry_forward_rows(&state, &user, &q).await?;
    let num = crate::routes::leaves::num;
    Ok(Json(
        rows.into_iter()
            .map(|r| CarryForwardRow {
                user: CarryForwardUser {
                    id: r.user_id,
                    full_name: r.full_name,
                    email: r.email,
                },
                leave_type: crate::routes::leaves::TypeRef {
                    id: r.leave_type_id,
                    code: r.code,
                    name: r.name,
                    color: r.color,
                },
                unused: num(r.unused),
                carry: num(r.carry),
                lapse: num(r.lapse),
            })
            .collect(),
    ))
}

#[utoipa::path(get, path = "/api/v1/admin/carry-forward/preview.csv", tag = "admin",
    params(("year" = Option<i32>, Query, description = "The leave year being closed; defaults to the current one")),
    responses(
        (status = 200, description = "`email,full_name,leave_type,unused,carry,lapse` (contracts/csv-formats.md §7)", content_type = "text/csv"),
        (status = 403, description = "Not an admin", body = crate::openapi::ErrorBody),
    ))]
pub async fn carry_forward_csv(
    State(state): State<AppState>,
    user: CurrentUser,
    Query(q): Query<CarryForwardQuery>,
) -> ApiResult<Response> {
    let (year, rows) = carry_forward_rows(&state, &user, &q).await?;
    Ok((
        StatusCode::OK,
        [
            (
                axum::http::header::CONTENT_TYPE,
                "text/csv; charset=utf-8".to_string(),
            ),
            (
                axum::http::header::CONTENT_DISPOSITION,
                format!("attachment; filename=\"carry-forward-{year}.csv\""),
            ),
        ],
        leavo_db::rollover_repo::report_csv(&rows),
    )
        .into_response())
}
