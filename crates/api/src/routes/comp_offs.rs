//! Comp-offs (T153, FR-030–FR-032, FR-060, FR-061): credit for working a past weekend day or
//! mandatory holiday, through the same approval flow as leave.
use crate::app::AppState;
use crate::auth::action_token::{self, Check};
use crate::auth::CurrentUser;
use crate::error::{field_error, ApiError, ApiResult};
use crate::extract::ApiJson;
use crate::middleware::idempotency::{self, Claim, IdempotencyKey};
use crate::routes::approvals::{ApprovalItem, WorkedDate};
use crate::routes::approvals::{AvailableBalance, DecideInput, Decided, Decision};
use crate::routes::leaves::{
    can_view_request, days_text, num, resolve_notify, NotifyInput, PersonRef, TypeRef,
};
use axum::extract::{ConnectInfo, Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use chrono::{DateTime, NaiveDate, Utc};
use leavo_core::comp_off::check_claim_date;
use leavo_core::error::ErrorCode;
use leavo_core::permissions::Role;
use leavo_core::policy::MAX_NOTIFY;
use leavo_db::audit::{write_audit, AuditEntry};
use leavo_db::comp_off_repo::{
    self as repo, CompOffRow, CreditRow, NewCompOffRequest, RevokeRefused,
};
use leavo_db::leaves_repo::{self, Notified};
use leavo_db::outbox::{notify, OutgoingEmail};
use leavo_db::tx::{begin_org_tx, lock_balance};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::PgConnection;
use std::collections::BTreeSet;
use std::net::SocketAddr;
use utoipa::ToSchema;
use uuid::Uuid;

/// Most dates in one request.
const MAX_DATES: usize = 31;

#[derive(Serialize, ToSchema, Clone, PartialEq, Debug)]
pub struct NextExpiry {
    pub date: NaiveDate,
    /// Spendable days that expire on that date.
    pub days: f64,
}

/// The soonest date on which spendable comp-off days expire.
pub(crate) fn next_expiry(credits: &[CreditRow]) -> Option<NextExpiry> {
    let date = credits
        .iter()
        .filter(|c| c.left() > Decimal::ZERO)
        .map(|c| c.expires_on)
        .min()?;
    let days: Decimal = credits
        .iter()
        .filter(|c| c.expires_on == date)
        .map(CreditRow::left)
        .sum();
    Some(NextExpiry {
        date,
        days: num(days),
    })
}

#[derive(Serialize, ToSchema)]
pub struct UsedOn {
    pub leave_request_id: Uuid,
    pub from: NaiveDate,
    pub to: NaiveDate,
    pub days: f64,
}

#[derive(Serialize, ToSchema)]
pub struct CreditItem {
    pub id: Uuid,
    pub worked_date: NaiveDate,
    pub days: f64,
    pub days_used: f64,
    /// Held by pending comp-off leave; never expires while held (FR-060).
    pub days_reserved: f64,
    pub days_expired: f64,
    /// `active`, `used`, `expired` or `revoked`.
    pub status: String,
    pub approved_by: Option<PersonRef>,
    /// The credit expires at the start of this date.
    pub expires_on: NaiveDate,
    pub used_on: Vec<UsedOn>,
    pub revoke_reason: Option<String>,
}

#[derive(Serialize, ToSchema)]
pub struct CompOffRequestItem {
    pub id: Uuid,
    /// `pending`, `approved`, `rejected` or `cancelled`.
    pub status: String,
    pub reason: String,
    pub dates: Vec<WorkedDate>,
    pub days: f64,
    pub requested_at: DateTime<Utc>,
    pub decided_by: Option<PersonRef>,
    pub decided_at: Option<DateTime<Utc>>,
    pub decision_comment: Option<String>,
    pub version: i32,
}

#[derive(Serialize, ToSchema)]
pub struct CompOffSummary {
    /// Comp-off days that can be spent now (one pool across leave years, FR-032).
    pub available: f64,
    pub next_expiry: Option<NextExpiry>,
    /// Soonest expiry first.
    pub credits: Vec<CreditItem>,
    /// Newest first.
    pub requests: Vec<CompOffRequestItem>,
}

fn credit_item(c: CreditRow, used: &[(Uuid, Uuid, NaiveDate, NaiveDate, Decimal)]) -> CreditItem {
    CreditItem {
        id: c.id,
        worked_date: c.worked_date,
        days: num(c.days),
        days_used: num(c.days_used),
        days_reserved: num(c.days_reserved),
        days_expired: num(c.days_expired),
        status: c.status().into(),
        approved_by: c
            .approved_by
            .zip(c.approved_by_name.clone())
            .map(|(id, full_name)| PersonRef { id, full_name }),
        expires_on: c.expires_on,
        used_on: used
            .iter()
            .filter(|u| u.0 == c.id)
            .map(|u| UsedOn {
                leave_request_id: u.1,
                from: u.2,
                to: u.3,
                days: num(u.4),
            })
            .collect(),
        revoke_reason: c.revoke_reason,
    }
}

async fn worked_dates(conn: &mut PgConnection, request: Uuid) -> ApiResult<Vec<WorkedDate>> {
    Ok(repo::dates_of(conn, request)
        .await?
        .into_iter()
        .map(|(date, portion)| WorkedDate {
            date,
            portion: num(portion),
        })
        .collect())
}

#[derive(Deserialize)]
pub struct SummaryQuery {
    /// Admins: another employee's comp-offs.
    pub user: Option<Uuid>,
}

#[utoipa::path(get, path = "/api/v1/comp-offs", tag = "comp-offs",
    params(("user" = Option<Uuid>, Query, description = "Admins only: another employee")),
    responses(
        (status = 200, description = "Comp-off balance, next expiry, credits and requests", body = CompOffSummary),
        (status = 401, description = "Not signed in", body = crate::openapi::ErrorBody),
        (status = 403, description = "FORBIDDEN: `user` is for admins", body = crate::openapi::ErrorBody),
        (status = 404, description = "NOT_FOUND: no such user", body = crate::openapi::ErrorBody),
    ))]
pub async fn summary(
    State(state): State<AppState>,
    user: CurrentUser,
    Query(q): Query<SummaryQuery>,
) -> ApiResult<Json<CompOffSummary>> {
    let subject = match q.user {
        Some(id) if id != user.id && user.role != Role::Admin => {
            return Err(ErrorCode::Forbidden.into())
        }
        Some(id) => id,
        None => user.id,
    };
    let mut tx = begin_org_tx(&state.db, user.org_id).await?;
    let exists: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM users WHERE id = $1)")
        .bind(subject)
        .fetch_one(&mut *tx)
        .await?;
    if !exists {
        return Err(ErrorCode::NotFound.into());
    }
    let available =
        match sqlx::query_scalar::<_, Uuid>("SELECT id FROM leave_types WHERE is_comp_off")
            .fetch_optional(&mut *tx)
            .await?
        {
            Some(t) => {
                leaves_repo::balance_row(&mut tx, subject, t, None)
                    .await?
                    .available
            }
            None => Decimal::ZERO,
        };
    let credits = repo::credits_of(&mut tx, subject).await?;
    let used = repo::used_on(&mut tx, subject).await?;
    let next = next_expiry(&credits);
    let mut requests = Vec::new();
    for r in repo::requests_of(&mut tx, subject).await? {
        requests.push(CompOffRequestItem {
            dates: worked_dates(&mut tx, r.id).await?,
            id: r.id,
            status: r.status,
            reason: r.reason,
            days: num(r.days),
            requested_at: r.created_at,
            decided_by: r
                .decided_by
                .zip(r.decided_by_name)
                .map(|(id, full_name)| PersonRef { id, full_name }),
            decided_at: r.decided_at,
            decision_comment: r.decision_comment,
            version: r.version,
        });
    }
    tx.commit().await?;
    Ok(Json(CompOffSummary {
        available: num(available),
        next_expiry: next,
        credits: credits.into_iter().map(|c| credit_item(c, &used)).collect(),
        requests,
    }))
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct WorkedDateInput {
    pub date: NaiveDate,
    /// 1 (full day) or 0.5 (half day).
    pub portion: f64,
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CompOffInput {
    /// 1–31 past weekend days or mandatory holidays, each once.
    pub dates: Vec<WorkedDateInput>,
    pub reason: String,
    #[serde(default)]
    pub notify: Vec<NotifyInput>,
}

#[derive(Serialize, ToSchema)]
pub struct CompOffCreated {
    pub id: Uuid,
    pub status: String,
    pub days: f64,
}

fn invalid_date(date: NaiveDate) -> ApiError {
    ApiError::new(ErrorCode::CompOffDateInvalid).with_details(json!({ "date": date }))
}

/// "Mon 9 Nov 2026, Sat 21 Nov 2026 (half day)".
fn dates_text(dates: &[(NaiveDate, Decimal)]) -> String {
    dates
        .iter()
        .map(|(d, p)| {
            let mut s = d.format("%a %-d %b %Y").to_string();
            if *p < Decimal::ONE {
                s.push_str(" (half day)");
            }
            s
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// The requester's name, email and the org name.
async fn people(conn: &mut PgConnection, user: Uuid) -> ApiResult<(String, String, String)> {
    Ok(sqlx::query_as(
        "SELECT u.full_name, u.email::text, o.name FROM users u
         JOIN organizations o ON o.id = u.org_id WHERE u.id = $1",
    )
    .bind(user)
    .fetch_one(conn)
    .await?)
}

async fn notified(conn: &mut PgConnection, request: Uuid) -> ApiResult<Vec<String>> {
    Ok(sqlx::query_scalar(
        "SELECT email::text FROM request_notifications
         WHERE request_kind = 'comp_off' AND request_id = $1 ORDER BY email",
    )
    .bind(request)
    .fetch_all(conn)
    .await?)
}

/// Notified members: name, dates, days and status only (FR-059).
async fn queue_fyi(
    conn: &mut PgConnection,
    org: Uuid,
    request: Uuid,
    context: serde_json::Value,
    now: DateTime<Utc>,
) -> ApiResult<()> {
    for to in notified(conn, request).await? {
        notify(
            conn,
            &OutgoingEmail {
                org_id: Some(org),
                template: "comp_off_fyi".into(),
                to,
                context: context.clone(),
                attachments: vec![],
            },
            now,
        )
        .await?;
    }
    Ok(())
}

#[utoipa::path(post, path = "/api/v1/comp-offs", tag = "comp-offs", request_body = CompOffInput,
    params(("Idempotency-Key" = String, Header, description = "UUID; a retry with the same key returns the first response")),
    responses(
        (status = 201, description = "Pending; the requester's approvers are emailed with action links", body = CompOffCreated),
        (status = 401, description = "Not signed in", body = crate::openapi::ErrorBody),
        (status = 409, description = "IDEMPOTENCY_MISMATCH", body = crate::openapi::ErrorBody),
        (status = 422, description = "COMP_OFF_DATE_INVALID (details.date: not past, not a weekend day or mandatory holiday, or already claimed); TYPE_NOT_AVAILABLE (no active comp-off type); NO_APPROVER_AVAILABLE; VALIDATION (details.field: dates, reason, notify)", body = crate::openapi::ErrorBody),
    ))]
pub async fn create(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    user: CurrentUser,
    key: IdempotencyKey,
    ApiJson(body): ApiJson<serde_json::Value>,
) -> ApiResult<Response> {
    let input: CompOffInput = serde_json::from_value(body.clone()).map_err(|e| {
        ApiError::new(ErrorCode::Validation).with_details(json!({ "body": e.to_string() }))
    })?;
    let mut dates: Vec<(NaiveDate, Decimal)> = Vec::with_capacity(input.dates.len());
    for d in &input.dates {
        let portion = match d.portion {
            1.0 => Decimal::ONE,
            0.5 => Decimal::new(5, 1),
            _ => return Err(field_error("dates")),
        };
        dates.push((d.date, portion));
    }
    dates.sort();
    let distinct: BTreeSet<NaiveDate> = dates.iter().map(|d| d.0).collect();
    if dates.is_empty() || dates.len() > MAX_DATES || distinct.len() != dates.len() {
        return Err(field_error("dates"));
    }
    let reason = input.reason.trim().to_string();
    if reason.is_empty() || reason.chars().count() > 500 {
        return Err(field_error("reason"));
    }
    if input.notify.len() > MAX_NOTIFY {
        return Err(field_error("notify"));
    }
    let now = state.clock.now();
    let mut tx = begin_org_tx(&state.db, user.org_id).await?;
    if let Claim::Replay(status, response) = idempotency::claim(
        &mut tx,
        user.org_id,
        user.id,
        &key,
        "POST /comp-offs",
        &idempotency::body_hash(&body),
        now,
    )
    .await?
    {
        tx.commit().await?;
        return Ok((status, Json(response)).into_response());
    }
    let notified_members: Vec<Notified> = resolve_notify(&mut tx, &input.notify).await?;
    if repo::comp_off_type(&mut tx).await?.is_none() {
        return Err(ErrorCode::TypeNotAvailable.into());
    }
    let cal = leaves_repo::org_calendar(&mut tx, user.org_id, now).await?;
    let (first, last) = (dates[0].0, dates[dates.len() - 1].0);
    let holidays = leaves_repo::holidays_between(&mut tx, first, last).await?;
    for (date, _) in &dates {
        check_claim_date(*date, cal.today, cal.weekend, &holidays)
            .map_err(|_| invalid_date(*date))?;
    }
    leaves_repo::lock_requester(&mut tx, user.id).await?;
    let all: Vec<NaiveDate> = dates.iter().map(|d| d.0).collect();
    if let Some(taken) = repo::claimed(&mut tx, user.id, &all).await?.first() {
        return Err(invalid_date(*taken));
    }
    let (requester, _, org_name) = people(&mut tx, user.id).await?;
    let group: Option<Uuid> =
        sqlx::query_scalar("SELECT approver_group_id FROM users WHERE id = $1")
            .bind(user.id)
            .fetch_one(&mut *tx)
            .await?;
    let approvers = leaves_repo::eligible_approvers(&mut tx, group, user.id)
        .await?
        .map_err(ApiError::from)?;
    let id = Uuid::now_v7();
    repo::insert_request(
        &mut tx,
        &NewCompOffRequest {
            id,
            org_id: user.org_id,
            user_id: user.id,
            reason: &reason,
            routed_group_id: group,
            now,
        },
        &dates,
    )
    .await
    .map_err(|e| {
        if leavo_db::orgs_repo::is_unique_violation(&e) {
            invalid_date(first)
        } else {
            e.into()
        }
    })?;
    leaves_repo::insert_notifications(&mut tx, user.org_id, id, "comp_off", &notified_members)
        .await?;

    let days: Decimal = dates.iter().map(|d| d.1).sum();
    let text = dates_text(&dates);
    let base = &state.config.base_url;
    let emails: Vec<(Uuid, String)> =
        sqlx::query_as("SELECT id, email::text FROM users WHERE id = ANY($1) ORDER BY email")
            .bind(&approvers)
            .fetch_all(&mut *tx)
            .await?;
    let secret = state.config.secret_key.as_bytes();
    for (approver, to) in emails {
        let mut urls = Vec::new();
        for decision in ["approve", "reject"] {
            let token = action_token::mint(
                &mut tx,
                secret,
                user.org_id,
                id,
                "comp_off",
                approver,
                decision,
                now,
            )
            .await?;
            urls.push(format!("{base}/api/v1/approvals/action/{token}"));
        }
        notify(
            &mut tx,
            &OutgoingEmail {
                org_id: Some(user.org_id),
                template: "comp_off_requested_action".into(),
                to,
                context: json!({
                    "org_name": org_name, "base_url": base, "requester": requester,
                    "dates": text, "days_text": days_text(days), "reason": reason,
                    "approve_url": urls[0], "reject_url": urls[1],
                }),
                attachments: vec![],
            },
            now,
        )
        .await?;
    }
    queue_fyi(
        &mut tx,
        user.org_id,
        id,
        json!({
            "org_name": org_name, "requester": requester, "dates": text,
            "days_text": days_text(days), "status": "pending",
        }),
        now,
    )
    .await?;
    write_audit(
        &mut tx,
        &AuditEntry {
            org_id: user.org_id,
            actor_id: Some(user.id),
            action: "comp_off.request",
            entity_kind: "comp_off_request",
            entity_id: Some(id),
            before: None,
            after: Some(json!({ "dates": all, "days": num(days), "reason": reason })),
            ip: Some(peer.ip()),
        },
    )
    .await?;
    let created = CompOffCreated {
        id,
        status: "pending".into(),
        days: num(days),
    };
    let response = serde_json::to_value(&created).map_err(ApiError::internal)?;
    idempotency::store(&mut tx, user.id, &key, StatusCode::CREATED, &response).await?;
    tx.commit().await?;
    Ok((StatusCode::CREATED, Json(response)).into_response())
}

fn already_decided(r: &CompOffRow) -> ApiError {
    ApiError::new(ErrorCode::AlreadyDecided).with_details(json!({
        "status": r.status,
        "decided_by": r.decided_by.map(|id| json!({ "id": id, "full_name": r.decided_by_name })),
        "decided_at": r.decided_at,
    }))
}

async fn reload(conn: &mut PgConnection, id: Uuid) -> ApiResult<CompOffRow> {
    repo::load(conn, id)
        .await?
        .ok_or_else(|| ApiError::new(ErrorCode::NotFound))
}

#[derive(Serialize, ToSchema)]
pub struct CompOffCancelled {
    pub status: String,
}

#[utoipa::path(post, path = "/api/v1/comp-offs/{id}/cancel", tag = "comp-offs",
    params(("id" = Uuid, Path, description = "Comp-off request id")),
    responses(
        (status = 200, description = "Withdrawn; its dates can be claimed again", body = CompOffCancelled),
        (status = 401, description = "Not signed in", body = crate::openapi::ErrorBody),
        (status = 403, description = "FORBIDDEN: only the requester withdraws", body = crate::openapi::ErrorBody),
        (status = 404, description = "NOT_FOUND", body = crate::openapi::ErrorBody),
        (status = 409, description = "ALREADY_DECIDED", body = crate::openapi::ErrorBody),
    ))]
pub async fn cancel(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    user: CurrentUser,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<CompOffCancelled>> {
    let now = state.clock.now();
    let mut tx = begin_org_tx(&state.db, user.org_id).await?;
    let r = reload(&mut tx, id).await?;
    if r.user_id != user.id {
        return Err(
            if can_view_request(&mut tx, &user, r.user_id, r.routed_group_id).await? {
                ErrorCode::Forbidden.into()
            } else {
                ErrorCode::NotFound.into()
            },
        );
    }
    if !repo::move_status(&mut tx, id, "cancelled", None, None, None, now).await? {
        return Err(already_decided(&reload(&mut tx, id).await?));
    }
    repo::free_dates(&mut tx, id).await?;
    leavo_db::approvals_repo::retire_action_tokens(&mut tx, id, now).await?;
    write_audit(
        &mut tx,
        &AuditEntry {
            org_id: user.org_id,
            actor_id: Some(user.id),
            action: "comp_off.withdraw",
            entity_kind: "comp_off_request",
            entity_id: Some(id),
            before: Some(json!({ "status": r.status, "version": r.version })),
            after: Some(json!({ "status": "cancelled" })),
            ip: Some(peer.ip()),
        },
    )
    .await?;
    tx.commit().await?;
    Ok(Json(CompOffCancelled {
        status: "cancelled".into(),
    }))
}

/// Pending comp-off requests `user` may decide, as approval items (oldest first).
pub(crate) async fn inbox_items(
    conn: &mut PgConnection,
    user: &CurrentUser,
) -> ApiResult<Vec<ApprovalItem>> {
    let Some(co) = sqlx::query_as::<_, (Uuid, String, String, String)>(
        "SELECT id, code, name, color FROM leave_types WHERE is_comp_off",
    )
    .fetch_optional(&mut *conn)
    .await?
    else {
        return Ok(vec![]);
    };
    let rows = repo::pending_for(conn, user.id, user.role == Role::Admin).await?;
    let mut items = Vec::with_capacity(rows.len());
    for r in rows {
        let (full_name, _, _) = people(conn, r.user_id).await?;
        let balance = leaves_repo::balance_row(conn, r.user_id, co.0, None).await?;
        items.push(ApprovalItem {
            id: r.id,
            kind: "comp_off".into(),
            requester: PersonRef {
                id: r.user_id,
                full_name,
            },
            leave_type: TypeRef {
                id: co.0,
                code: co.1.clone(),
                name: co.2.clone(),
                color: co.3.clone(),
            },
            from: r.first_date,
            to: r.last_date,
            start_half: None,
            end_half: None,
            days: num(r.days),
            reason: r.reason,
            requested_at: r.created_at,
            version: r.version,
            attachment: None,
            balance: AvailableBalance {
                available: num(balance.available),
            },
            team_away: vec![],
            cancel: None,
            dates: Some(worked_dates(conn, r.id).await?),
        });
    }
    Ok(items)
}

/// `POST /approvals/comp_off/{id}/decide` (called from `approvals::decide`).
pub(crate) async fn decide(
    state: AppState,
    peer: SocketAddr,
    user: CurrentUser,
    key: IdempotencyKey,
    id: Uuid,
    body: serde_json::Value,
) -> ApiResult<Response> {
    let input: DecideInput = serde_json::from_value(body.clone()).map_err(|e| {
        ApiError::new(ErrorCode::Validation).with_details(json!({ "body": e.to_string() }))
    })?;
    let now = state.clock.now();
    let mut tx = begin_org_tx(&state.db, user.org_id).await?;
    if let Claim::Replay(status, response) = idempotency::claim(
        &mut tx,
        user.org_id,
        user.id,
        &key,
        &format!("POST /approvals/comp_off/{id}/decide"),
        &idempotency::body_hash(&body),
        now,
    )
    .await?
    {
        tx.commit().await?;
        return Ok((status, Json(response)).into_response());
    }
    let r = reload(&mut tx, id).await?;
    if r.user_id == user.id {
        return Err(ErrorCode::SelfApproval.into());
    }
    let eligible = match user.role {
        Role::Admin => true,
        Role::Approver => leaves_repo::eligible_approvers(&mut tx, r.routed_group_id, r.user_id)
            .await?
            .map(|ids| ids.contains(&user.id))
            .unwrap_or(false),
        Role::Employee => false,
    };
    if !eligible {
        return Err(ErrorCode::NotEligibleApprover.into());
    }
    if let Some(token) = &input.action_token {
        let ok = match action_token::check(&mut tx, state.config.secret_key.as_bytes(), token, now)
            .await?
        {
            Check::Valid(t) => {
                t.org_id == user.org_id
                    && t.request_id == id
                    && t.request_kind == "comp_off"
                    && t.approver_id == user.id
                    && t.decision
                        == match input.decision {
                            Decision::Approve => "approve",
                            Decision::Reject => "reject",
                        }
            }
            _ => false,
        };
        if !ok {
            return Err(ErrorCode::Forbidden.into());
        }
    }
    let comment = input
        .comment
        .as_deref()
        .map(str::trim)
        .filter(|c| !c.is_empty());
    if comment.is_some_and(|c| c.chars().count() > 500) {
        return Err(field_error("comment"));
    }
    let status = match input.decision {
        Decision::Reject if comment.is_none() => return Err(ErrorCode::CommentRequired.into()),
        Decision::Reject => "rejected",
        Decision::Approve => "approved",
    };
    if r.status != "pending" {
        return Err(already_decided(&r));
    }
    if let Some(co) = repo::comp_off_type(&mut tx).await? {
        lock_balance(&mut tx, r.user_id, co).await?;
    }
    if !repo::move_status(
        &mut tx,
        id,
        status,
        Some(input.version),
        Some(user.id),
        comment,
        now,
    )
    .await?
    {
        return Err(already_decided(&reload(&mut tx, id).await?));
    }
    let credited = if status == "approved" {
        Some(repo::create_credits(&mut tx, &r, user.id, now).await?)
    } else {
        repo::free_dates(&mut tx, id).await?;
        None
    };
    leavo_db::approvals_repo::retire_action_tokens(&mut tx, id, now).await?;

    let (requester, email, org_name) = people(&mut tx, r.user_id).await?;
    let (decider, _, _) = people(&mut tx, user.id).await?;
    let dates = repo::dates_of(&mut tx, id).await?;
    let text = dates_text(&dates);
    let (expiry_days,): (i32,) =
        sqlx::query_as("SELECT comp_off_expiry_days FROM organizations WHERE id = $1")
            .bind(user.org_id)
            .fetch_one(&mut *tx)
            .await?;
    notify(
        &mut tx,
        &OutgoingEmail {
            org_id: Some(user.org_id),
            template: "comp_off_decided".into(),
            to: email,
            context: json!({
                "org_name": org_name, "base_url": state.config.base_url, "requester": requester,
                "dates": text, "days_text": days_text(r.days), "status": status,
                "decided_by": decider, "comment": comment, "expiry_days": expiry_days,
            }),
            attachments: vec![],
        },
        now,
    )
    .await?;
    queue_fyi(
        &mut tx,
        user.org_id,
        id,
        json!({
            "org_name": org_name, "requester": requester, "dates": text,
            "days_text": days_text(r.days), "status": status,
        }),
        now,
    )
    .await?;
    write_audit(
        &mut tx,
        &AuditEntry {
            org_id: user.org_id,
            actor_id: Some(user.id),
            action: if credited.is_some() {
                "comp_off.approve"
            } else {
                "comp_off.reject"
            },
            entity_kind: "comp_off_request",
            entity_id: Some(id),
            before: Some(json!({ "status": r.status, "version": r.version })),
            after: Some(json!({
                "status": status, "comment": comment,
                "credited": credited.map(|d| d.to_string()),
            })),
            ip: Some(peer.ip()),
        },
    )
    .await?;
    let decided = Decided {
        status: status.into(),
        decided_by: PersonRef {
            id: user.id,
            full_name: decider,
        },
        decided_at: now,
    };
    let response = serde_json::to_value(&decided).map_err(ApiError::internal)?;
    idempotency::store(&mut tx, user.id, &key, StatusCode::OK, &response).await?;
    tx.commit().await?;
    Ok((StatusCode::OK, Json(response)).into_response())
}

#[derive(Deserialize, ToSchema)]
pub struct RevokeInput {
    /// Required, up to 500 characters; recorded on the ledger entry.
    pub reason: Option<String>,
}

#[utoipa::path(post, path = "/api/v1/admin/comp-off-credits/{id}/revoke", tag = "comp-offs",
    request_body = RevokeInput,
    params(("id" = Uuid, Path, description = "Credit id (from `GET /comp-offs?user=`)")),
    responses(
        (status = 200, description = "Revoked: `comp_off_revoked` for the days not yet expired", body = CreditItem),
        (status = 401, description = "Not signed in", body = crate::openapi::ErrorBody),
        (status = 403, description = "FORBIDDEN: admins only", body = crate::openapi::ErrorBody),
        (status = 404, description = "NOT_FOUND", body = crate::openapi::ErrorBody),
        (status = 409, description = "ALREADY_DECIDED: already revoked", body = crate::openapi::ErrorBody),
        (status = 422, description = "NOTE_REQUIRED; CREDIT_IN_USE (used or reserved); VALIDATION (details.field: reason)", body = crate::openapi::ErrorBody),
    ))]
pub async fn revoke(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    user: CurrentUser,
    Path(id): Path<Uuid>,
    ApiJson(body): ApiJson<serde_json::Value>,
) -> ApiResult<Json<CreditItem>> {
    if user.role != Role::Admin {
        return Err(ErrorCode::Forbidden.into());
    }
    let input: RevokeInput = serde_json::from_value(body).map_err(|e| {
        ApiError::new(ErrorCode::Validation).with_details(json!({ "body": e.to_string() }))
    })?;
    let reason = input.reason.as_deref().map(str::trim).unwrap_or("");
    if reason.is_empty() {
        return Err(ErrorCode::NoteRequired.into());
    }
    if reason.chars().count() > 500 {
        return Err(field_error("reason"));
    }
    let now = state.clock.now();
    let mut tx = begin_org_tx(&state.db, user.org_id).await?;
    let today = leaves_repo::org_calendar(&mut tx, user.org_id, now)
        .await?
        .today;
    let credit = match repo::revoke(&mut tx, user.org_id, id, user.id, reason, now, today).await? {
        Ok(c) => c,
        Err(RevokeRefused::NotFound) => return Err(ErrorCode::NotFound.into()),
        Err(RevokeRefused::AlreadyRevoked) => return Err(ErrorCode::AlreadyDecided.into()),
        Err(RevokeRefused::InUse) => return Err(ErrorCode::CreditInUse.into()),
    };
    write_audit(
        &mut tx,
        &AuditEntry {
            org_id: user.org_id,
            actor_id: Some(user.id),
            action: "comp_off.revoke",
            entity_kind: "comp_off_credit",
            entity_id: Some(id),
            before: Some(json!({ "user_id": credit.user_id, "days": credit.days.to_string() })),
            after: Some(json!({ "status": "revoked", "reason": reason })),
            ip: Some(peer.ip()),
        },
    )
    .await?;
    tx.commit().await?;
    Ok(Json(credit_item(credit, &[])))
}
