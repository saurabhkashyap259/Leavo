//! Cancelling leave (T141, FR-018, FR-020, FR-046, FR-059): the employee withdraws a pending
//! request, cancels approved leave before it starts, or asks an approver to accept cancelling
//! leave already under way. Accepting and declining live in `approvals::decide` (`kind=cancel`).
use crate::app::AppState;
use crate::auth::CurrentUser;
use crate::error::{field_error, ApiError, ApiResult};
use crate::extract::ApiJson;
use crate::routes::approvals::{already_decided, core_half, type_ref};
use crate::routes::leaves::{can_view_request, dates_text, days_text};
use axum::extract::{ConnectInfo, Path, State};
use axum::Json;
use chrono::{DateTime, NaiveDate, Utc};
use leavo_core::approval::{cancel_event, days_to_return, RequestEvent, RequestStatus};
use leavo_core::error::ErrorCode;
use leavo_db::approvals_repo::{self as repo, RequestRow};
use leavo_db::audit::{write_audit, AuditEntry};
use leavo_db::outbox::{notify, OutgoingEmail};
use leavo_db::tx::{begin_org_tx, lock_balance};
use leavo_db::{comp_off_repo, leaves_repo};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::PgConnection;
use std::net::SocketAddr;
use utoipa::ToSchema;
use uuid::Uuid;

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CancelInput {
    /// Optional, up to 500 characters; shown to approvers.
    pub reason: Option<String>,
}

#[derive(Serialize, ToSchema)]
pub struct CancelResult {
    /// `cancelled`, or `cancel_requested` when the leave has started and an approver must accept.
    pub status: String,
}

/// The org-local date a cancellation was requested on: days from then on are returned.
pub(crate) async fn requested_on(conn: &mut PgConnection, r: &RequestRow) -> ApiResult<NaiveDate> {
    let tz = leaves_repo::org_timezone(conn, r.org_id).await?;
    let at = r
        .cancel_requested_at
        .ok_or_else(|| ApiError::internal("cancel_requested without a time"))?;
    Ok(at.with_timezone(&tz).date_naive())
}

/// Days a pending cancellation would return.
pub(crate) async fn days_returned(conn: &mut PgConnection, r: &RequestRow) -> ApiResult<Decimal> {
    let from = requested_on(conn, r).await?;
    let days = repo::active_days(conn, r.id).await?;
    Ok(days_to_return(&days, from).iter().map(|(_, d)| d).sum())
}

/// The requester's name, email and the org name.
async fn people(conn: &mut PgConnection, r: &RequestRow) -> ApiResult<(String, String, String)> {
    Ok(sqlx::query_as(
        "SELECT u.full_name, u.email::text, o.name FROM users u
         JOIN organizations o ON o.id = u.org_id WHERE u.id = $1",
    )
    .bind(r.user_id)
    .fetch_one(conn)
    .await?)
}

/// The request's eligible approvers' emails (FR-022); none when nobody is left to route to.
async fn approver_emails(conn: &mut PgConnection, r: &RequestRow) -> ApiResult<Vec<String>> {
    let ids = leaves_repo::eligible_approvers(conn, r.routed_group_id, r.user_id)
        .await?
        .unwrap_or_default();
    Ok(
        sqlx::query_scalar("SELECT email::text FROM users WHERE id = ANY($1) ORDER BY email")
            .bind(&ids)
            .fetch_all(conn)
            .await?,
    )
}

fn dates(r: &RequestRow) -> String {
    dates_text(
        r.from_date,
        r.to_date,
        core_half(&r.start_half),
        core_half(&r.end_half),
    )
}

/// Cancelled: the employee, the request's approvers and its notified members hear about it.
/// Notified members get name, dates, days and status only (FR-046, FR-059).
pub(crate) async fn queue_cancelled_emails(
    conn: &mut PgConnection,
    state: &AppState,
    r: &RequestRow,
    returned: Decimal,
    accepted_by: Option<&str>,
    reason: Option<&str>,
    now: DateTime<Utc>,
) -> ApiResult<()> {
    let (requester, email, org_name) = people(conn, r).await?;
    let t = type_ref(conn, r.leave_type_id).await?;
    let dates = dates(r);
    let kept = r.days - returned;
    let context = |to_requester: bool| {
        json!({
            "org_name": org_name, "base_url": state.config.base_url, "requester": requester,
            "leave_type": t.name, "dates": dates, "days_text": days_text(r.days),
            "returned_text": days_text(returned),
            "kept_text": (kept > Decimal::ZERO).then(|| days_text(kept)),
            "accepted_by": accepted_by, "reason": reason, "to_requester": to_requester,
        })
    };
    let mut mails = vec![(email, context(true))];
    for to in approver_emails(conn, r).await? {
        mails.push((to, context(false)));
    }
    for (to, context) in mails {
        notify(
            conn,
            &OutgoingEmail {
                org_id: Some(r.org_id),
                template: "leave_cancelled".into(),
                to,
                context,
                attachments: vec![],
            },
            now,
        )
        .await?;
    }
    for (to, _) in repo::notified(conn, r.id).await? {
        notify(
            conn,
            &OutgoingEmail {
                org_id: Some(r.org_id),
                template: "leave_decided_fyi".into(),
                to,
                context: json!({
                    "org_name": org_name, "requester": requester, "dates": dates,
                    "days_text": days_text(r.days), "status": "cancelled",
                }),
                attachments: vec![],
            },
            now,
        )
        .await?;
    }
    Ok(())
}

/// Started leave: the request's approvers are asked to accept or decline.
async fn queue_cancel_requested_emails(
    conn: &mut PgConnection,
    state: &AppState,
    r: &RequestRow,
    today: NaiveDate,
    returned: Decimal,
    reason: Option<&str>,
    now: DateTime<Utc>,
) -> ApiResult<()> {
    let (requester, _, org_name) = people(conn, r).await?;
    let t = type_ref(conn, r.leave_type_id).await?;
    let dates = dates(r);
    for to in approver_emails(conn, r).await? {
        notify(
            conn,
            &OutgoingEmail {
                org_id: Some(r.org_id),
                template: "cancel_requested".into(),
                to,
                context: json!({
                    "org_name": org_name, "base_url": state.config.base_url,
                    "requester": requester, "leave_type": t.name, "dates": dates,
                    "days_text": days_text(r.days), "returned_text": days_text(returned),
                    "requested_on": today.format("%a %-d %b").to_string(), "reason": reason,
                    "inbox_url": format!("{}/approvals?kind=cancel", state.config.base_url),
                }),
                attachments: vec![],
            },
            now,
        )
        .await?;
    }
    Ok(())
}

/// Declined: the employee is told the leave stays approved.
pub(crate) async fn queue_declined_email(
    conn: &mut PgConnection,
    state: &AppState,
    r: &RequestRow,
    decided_by: &str,
    comment: Option<&str>,
    now: DateTime<Utc>,
) -> ApiResult<()> {
    let (requester, email, org_name) = people(conn, r).await?;
    let t = type_ref(conn, r.leave_type_id).await?;
    notify(
        conn,
        &OutgoingEmail {
            org_id: Some(r.org_id),
            template: "cancel_declined".into(),
            to: email,
            context: json!({
                "org_name": org_name, "base_url": state.config.base_url, "requester": requester,
                "leave_type": t.name, "dates": dates(r), "days_text": days_text(r.days),
                "decided_by": decided_by, "comment": comment,
            }),
            attachments: vec![],
        },
        now,
    )
    .await?;
    Ok(())
}

#[utoipa::path(post, path = "/api/v1/leaves/{id}/cancel", tag = "leaves",
    request_body = CancelInput,
    params(("id" = Uuid, Path, description = "Request id")),
    responses(
        (status = 200, description = "Pending: withdrawn and the days released. Approved and not started: cancelled and every day returned. Started: `cancel_requested` until an approver accepts (only days from the request date on are then returned) or declines", body = CancelResult),
        (status = 401, description = "Not signed in", body = crate::openapi::ErrorBody),
        (status = 403, description = "FORBIDDEN: only the requester cancels", body = crate::openapi::ErrorBody),
        (status = 404, description = "NOT_FOUND", body = crate::openapi::ErrorBody),
        (status = 409, description = "ALREADY_DECIDED: rejected, cancelled or already waiting for an approver; details { status, decided_by, decided_at }", body = crate::openapi::ErrorBody),
        (status = 422, description = "VALIDATION (details.field: reason)", body = crate::openapi::ErrorBody),
    ))]
pub async fn cancel(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    user: CurrentUser,
    Path(id): Path<Uuid>,
    ApiJson(body): ApiJson<serde_json::Value>,
) -> ApiResult<Json<CancelResult>> {
    let input: CancelInput = serde_json::from_value(body).map_err(|e| {
        ApiError::new(ErrorCode::Validation).with_details(json!({ "body": e.to_string() }))
    })?;
    let reason = input
        .reason
        .as_deref()
        .map(str::trim)
        .filter(|r| !r.is_empty());
    if reason.is_some_and(|r| r.chars().count() > 500) {
        return Err(field_error("reason"));
    }
    let now = state.clock.now();
    let mut tx = begin_org_tx(&state.db, user.org_id).await?;
    let r = repo::load(&mut tx, id)
        .await?
        .ok_or_else(|| ApiError::new(ErrorCode::NotFound))?;
    if r.user_id != user.id {
        return Err(
            if can_view_request(&mut tx, &user, r.user_id, r.routed_group_id).await? {
                ErrorCode::Forbidden.into()
            } else {
                ErrorCode::NotFound.into()
            },
        );
    }
    let today = leaves_repo::org_calendar(&mut tx, user.org_id, now)
        .await?
        .today;
    let current = RequestStatus::parse(&r.status).unwrap_or(RequestStatus::Pending);
    let Ok(event) = cancel_event(current, r.from_date, today) else {
        return Err(already_decided(&mut tx, id).await?);
    };
    lock_balance(&mut tx, r.user_id, r.leave_type_id).await?;
    let (to, action) = match event {
        RequestEvent::Withdraw => (RequestStatus::Cancelled, "leave.withdraw"),
        RequestEvent::CancelBeforeStart => (RequestStatus::Cancelled, "leave.cancel"),
        _ => (RequestStatus::CancelRequested, "leave.cancel_request"),
    };
    if !repo::move_status(&mut tx, id, &r.status, to.as_str(), None, reason, now).await? {
        return Err(already_decided(&mut tx, id).await?);
    }
    let returned = match event {
        RequestEvent::Withdraw => {
            repo::release_days(&mut tx, id).await?;
            repo::retire_action_tokens(&mut tx, id, now).await?;
            comp_off_repo::release(&mut tx, id, Some(user.id), now, today).await?;
            r.days
        }
        RequestEvent::CancelBeforeStart => {
            let returned: Decimal = repo::reverse_from(&mut tx, &r, user.id, r.from_date)
                .await?
                .iter()
                .map(|(_, d)| d)
                .sum();
            comp_off_repo::give_back(&mut tx, id, returned, Some(user.id), now, today).await?;
            returned
        }
        _ => {
            let days = repo::active_days(&mut tx, id).await?;
            days_to_return(&days, today).iter().map(|(_, d)| d).sum()
        }
    };
    if to == RequestStatus::Cancelled {
        queue_cancelled_emails(&mut tx, &state, &r, returned, None, reason, now).await?;
    } else {
        queue_cancel_requested_emails(&mut tx, &state, &r, today, returned, reason, now).await?;
    }
    write_audit(
        &mut tx,
        &AuditEntry {
            org_id: user.org_id,
            actor_id: Some(user.id),
            action,
            entity_kind: "leave_request",
            entity_id: Some(id),
            before: Some(json!({ "status": r.status, "version": r.version })),
            after: Some(json!({
                "status": to.as_str(), "reason": reason,
                "days_returned": (to == RequestStatus::Cancelled).then(|| returned.to_string()),
            })),
            ip: Some(peer.ip()),
        },
    )
    .await?;
    tx.commit().await?;
    Ok(Json(CancelResult {
        status: to.as_str().into(),
    }))
}
