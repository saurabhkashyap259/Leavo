//! Approvals (T120): inbox, decisions and email action links (FR-022–FR-024, FR-046, FR-058).
use crate::app::AppState;
use crate::auth::action_token::{self, Check};
use crate::auth::CurrentUser;
use crate::error::{field_error, ApiError, ApiResult};
use crate::extract::ApiJson;
use crate::middleware::idempotency::{self, Claim, IdempotencyKey};
use crate::routes::leaves::{
    dates_text, days_text, num, AttachmentRef, DayHalf, PersonRef, TypeRef,
};
use crate::routes::{cancel, comp_offs};
use axum::extract::{ConnectInfo, Path, Query, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use chrono::{DateTime, NaiveDate, Utc};
use leavo_core::approval::{transition, RequestEvent, RequestStatus};
use leavo_core::calendar::{leave_year_of, Half};
use leavo_core::error::ErrorCode;
use leavo_core::permissions::Role;
use leavo_core::visibility::{can_see_details, Subject, Viewer};
use leavo_db::approvals_repo::{self as repo, RequestRow};
use leavo_db::audit::{write_audit, AuditEntry};
use leavo_db::outbox::{notify, OutboxAttachment, OutgoingEmail};
use leavo_db::tx::{begin_org_tx, lock_balance};
use leavo_db::{comp_off_repo, leave_types_repo, leaves_repo};
use leavo_email::ics::{leave_event, ICS_CONTENT_TYPE, ICS_FILENAME};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::PgConnection;
use std::net::SocketAddr;
use utoipa::ToSchema;
use uuid::Uuid;

pub(crate) fn half(h: &Option<String>) -> Option<DayHalf> {
    match h.as_deref() {
        Some("first") => Some(DayHalf::First),
        Some("second") => Some(DayHalf::Second),
        _ => None,
    }
}

pub(crate) fn core_half(h: &Option<String>) -> Option<Half> {
    half(h).map(Into::into)
}

#[derive(Serialize, ToSchema)]
pub struct AvailableBalance {
    /// Available days of the request's type in the leave year it starts in.
    pub available: f64,
}

/// A teammate away on the request's dates. `leave_type` and `reason` are present only when the
/// viewer is an admin or an eligible approver of that teammate (FR-058).
#[derive(Serialize, ToSchema)]
pub struct TeamAwayItem {
    pub full_name: String,
    pub from: NaiveDate,
    pub to: NaiveDate,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub leave_type: Option<TypeRef>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

#[derive(Serialize, ToSchema)]
pub struct ApprovalItem {
    pub id: Uuid,
    /// `leave`, `cancel` (a request to cancel started leave) or `comp_off` (credit for worked days).
    pub kind: String,
    pub requester: PersonRef,
    pub leave_type: TypeRef,
    pub from: NaiveDate,
    pub to: NaiveDate,
    pub start_half: Option<DayHalf>,
    pub end_half: Option<DayHalf>,
    pub days: f64,
    pub reason: String,
    pub requested_at: DateTime<Utc>,
    /// Send back with the decision (optimistic locking).
    pub version: i32,
    pub attachment: Option<AttachmentRef>,
    pub balance: AvailableBalance,
    pub team_away: Vec<TeamAwayItem>,
    /// Present on `kind=cancel` items: the employee asks to cancel leave that has started.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cancel: Option<CancelInfo>,
    /// Present on `kind=comp_off` items: the worked dates claimed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dates: Option<Vec<WorkedDate>>,
}

/// A worked date claimed for comp-off credit (FR-030).
#[derive(Serialize, ToSchema)]
pub struct WorkedDate {
    pub date: NaiveDate,
    /// 1 or 0.5.
    pub portion: f64,
}

/// A request to cancel leave that has already started (FR-020).
#[derive(Serialize, ToSchema)]
pub struct CancelInfo {
    pub requested_at: DateTime<Utc>,
    pub reason: Option<String>,
    /// Days that go back to the balance if accepted: those dated on or after the request date.
    pub days_returned: f64,
}

#[derive(Deserialize)]
pub struct InboxQuery {
    pub status: Option<String>,
    pub kind: Option<String>,
}

fn require_approver(user: &CurrentUser) -> ApiResult<()> {
    match user.role {
        Role::Approver | Role::Admin => Ok(()),
        Role::Employee => Err(ErrorCode::Forbidden.into()),
    }
}

pub(crate) async fn type_ref(conn: &mut PgConnection, id: Uuid) -> ApiResult<TypeRef> {
    let (code, name, color): (String, String, String) =
        sqlx::query_as("SELECT code, name, color FROM leave_types WHERE id = $1")
            .bind(id)
            .fetch_one(conn)
            .await?;
    Ok(TypeRef {
        id,
        code,
        name,
        color,
    })
}

async fn full_name(conn: &mut PgConnection, id: Uuid) -> ApiResult<String> {
    Ok(
        sqlx::query_scalar("SELECT full_name FROM users WHERE id = $1")
            .bind(id)
            .fetch_one(conn)
            .await?,
    )
}

#[utoipa::path(get, path = "/api/v1/approvals", tag = "approvals",
    params(
        ("status" = Option<String>, Query, description = "`pending` (default)"),
        ("kind" = Option<String>, Query, description = "`leave` (default), `cancel` or `comp_off`"),
    ),
    responses(
        (status = 200, description = "Requests the viewer may decide, oldest first", body = [ApprovalItem]),
        (status = 401, description = "Not signed in", body = crate::openapi::ErrorBody),
        (status = 403, description = "FORBIDDEN: employees have no inbox", body = crate::openapi::ErrorBody),
    ))]
pub async fn inbox(
    State(state): State<AppState>,
    user: CurrentUser,
    Query(q): Query<InboxQuery>,
) -> ApiResult<Json<Vec<ApprovalItem>>> {
    require_approver(&user)?;
    let kind = q.kind.as_deref().unwrap_or("leave");
    if q.status.as_deref().is_some_and(|s| s != "pending")
        || !["leave", "cancel", "comp_off"].contains(&kind)
    {
        return Ok(Json(vec![]));
    }
    let mut tx = begin_org_tx(&state.db, user.org_id).await?;
    if kind == "comp_off" {
        let items = comp_offs::inbox_items(&mut tx, &user).await?;
        tx.commit().await?;
        return Ok(Json(items));
    }
    let cal = leaves_repo::org_calendar(&mut tx, user.org_id, state.clock.now()).await?;
    let is_admin = user.role == Role::Admin;
    let my_groups = repo::groups_of(&mut tx, user.id).await?;
    let viewer = Viewer {
        id: user.id,
        role: user.role,
        approves: &my_groups,
    };
    let rows = if kind == "cancel" {
        repo::cancel_requests_for(&mut tx, user.id, is_admin).await?
    } else {
        repo::pending_for(&mut tx, user.id, is_admin).await?
    };
    let mut items = Vec::with_capacity(rows.len());
    for r in rows {
        let t = leave_types_repo::get(&mut tx, r.leave_type_id)
            .await?
            .ok_or_else(|| ApiError::new(ErrorCode::NotFound))?;
        let year = leave_year_of(r.from_date, cal.start_month);
        let balance =
            leaves_repo::balance_with_projection(&mut tx, r.user_id, &t, year, &cal).await?;
        let attachment: Option<(Uuid, String)> =
            sqlx::query_as("SELECT id, file_name FROM attachments WHERE request_id = $1")
                .bind(r.id)
                .fetch_optional(&mut *tx)
                .await?;
        let team_away = repo::team_away(&mut tx, r.user_id, r.from_date, r.to_date)
            .await?
            .into_iter()
            .map(|a| {
                // Person-level eligibility: the teammate's current group (FR-058).
                let eligible = can_see_details(
                    &viewer,
                    &Subject {
                        user: a.user_id,
                        group: a.group_id,
                    },
                );
                TeamAwayItem {
                    full_name: a.full_name,
                    from: a.from_date,
                    to: a.to_date,
                    leave_type: eligible.then_some(TypeRef {
                        id: a.leave_type_id,
                        code: a.code,
                        name: a.name,
                        color: a.color,
                    }),
                    reason: eligible.then_some(a.reason),
                }
            })
            .collect();
        let cancel = match r.cancel_requested_at {
            Some(requested_at) if kind == "cancel" => Some(CancelInfo {
                requested_at,
                reason: r.cancel_reason.clone(),
                days_returned: num(cancel::days_returned(&mut tx, &r).await?),
            }),
            _ => None,
        };
        items.push(ApprovalItem {
            id: r.id,
            kind: kind.into(),
            requester: PersonRef {
                id: r.user_id,
                full_name: full_name(&mut tx, r.user_id).await?,
            },
            leave_type: TypeRef {
                id: t.id,
                code: t.code,
                name: t.name,
                color: t.color,
            },
            from: r.from_date,
            to: r.to_date,
            start_half: half(&r.start_half),
            end_half: half(&r.end_half),
            days: num(r.days),
            reason: r.reason,
            requested_at: r.created_at,
            version: r.version,
            attachment: attachment.map(|(id, name)| AttachmentRef { id, name }),
            balance: AvailableBalance {
                available: num(balance.available),
            },
            team_away,
            cancel,
            dates: None,
        });
    }
    tx.commit().await?;
    Ok(Json(items))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum Decision {
    Approve,
    Reject,
}

impl Decision {
    fn as_str(self) -> &'static str {
        match self {
            Decision::Approve => "approve",
            Decision::Reject => "reject",
        }
    }
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct DecideInput {
    pub decision: Decision,
    /// Required to reject.
    pub comment: Option<String>,
    /// The `version` from the inbox or request detail.
    pub version: i32,
    /// The token from an email link, when the decision started there.
    pub action_token: Option<String>,
}

#[derive(Serialize, ToSchema)]
pub struct Decided {
    /// `approved` or `rejected`.
    pub status: String,
    pub decided_by: PersonRef,
    pub decided_at: DateTime<Utc>,
}

pub(crate) async fn already_decided(conn: &mut PgConnection, id: Uuid) -> ApiResult<ApiError> {
    let d = repo::decided(conn, id).await?;
    Ok(ApiError::new(ErrorCode::AlreadyDecided).with_details(json!({
        "status": d.status,
        "decided_by": d.decided_by.map(|id| json!({ "id": id, "full_name": d.decided_by_name })),
        "decided_at": d.decided_at,
    })))
}

/// Can `user` decide this request (FR-022)? Admins always; approvers when in its fixed set.
async fn eligible(conn: &mut PgConnection, user: &CurrentUser, r: &RequestRow) -> ApiResult<bool> {
    if user.role == Role::Admin {
        return Ok(true);
    }
    if user.role != Role::Approver {
        return Ok(false);
    }
    Ok(
        leaves_repo::eligible_approvers(conn, r.routed_group_id, r.user_id)
            .await?
            .map(|ids| ids.contains(&user.id))
            .unwrap_or(false),
    )
}

#[utoipa::path(post, path = "/api/v1/approvals/{kind}/{id}/decide", tag = "approvals",
    request_body = DecideInput,
    params(
        ("kind" = String, Path, description = "`leave`"),
        ("id" = Uuid, Path, description = "Request id"),
        ("Idempotency-Key" = String, Header, description = "UUID"),
    ),
    responses(
        (status = 200, description = "Decided. Approval deducts the days per leave year; rejection releases them", body = Decided),
        (status = 401, description = "Not signed in", body = crate::openapi::ErrorBody),
        (status = 403, description = "SELF_APPROVAL, NOT_ELIGIBLE_APPROVER, or FORBIDDEN for an action_token that is not this approver's valid link for this decision", body = crate::openapi::ErrorBody),
        (status = 404, description = "NOT_FOUND", body = crate::openapi::ErrorBody),
        (status = 409, description = "ALREADY_DECIDED with details { status, decided_by, decided_at }; IDEMPOTENCY_MISMATCH", body = crate::openapi::ErrorBody),
        (status = 422, description = "COMMENT_REQUIRED; VALIDATION", body = crate::openapi::ErrorBody),
    ))]
pub async fn decide(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    user: CurrentUser,
    key: IdempotencyKey,
    Path((kind, id)): Path<(String, Uuid)>,
    ApiJson(body): ApiJson<serde_json::Value>,
) -> ApiResult<Response> {
    let cancelling = match kind.as_str() {
        "leave" => false,
        "cancel" => true,
        "comp_off" => return comp_offs::decide(state, peer, user, key, id, body).await,
        _ => return Err(ErrorCode::NotFound.into()),
    };
    let input: DecideInput = serde_json::from_value(body.clone()).map_err(|e| {
        ApiError::new(ErrorCode::Validation).with_details(json!({ "body": e.to_string() }))
    })?;
    let now = state.clock.now();
    let mut tx = begin_org_tx(&state.db, user.org_id).await?;
    let endpoint = format!("POST /approvals/{kind}/{id}/decide");
    if let Claim::Replay(status, response) = idempotency::claim(
        &mut tx,
        user.org_id,
        user.id,
        &key,
        &endpoint,
        &idempotency::body_hash(&body),
        now,
    )
    .await?
    {
        tx.commit().await?;
        return Ok((status, Json(response)).into_response());
    }

    let r = repo::load(&mut tx, id)
        .await?
        .ok_or_else(|| ApiError::new(ErrorCode::NotFound))?;
    if r.user_id == user.id {
        return Err(ErrorCode::SelfApproval.into());
    }
    if !eligible(&mut tx, &user, &r).await? {
        return Err(ErrorCode::NotEligibleApprover.into());
    }
    if let Some(token) = &input.action_token {
        let ok = match action_token::check(&mut tx, state.config.secret_key.as_bytes(), token, now)
            .await?
        {
            Check::Valid(t) => {
                t.org_id == user.org_id
                    && t.request_id == id
                    && t.request_kind == kind
                    && t.approver_id == user.id
                    && t.decision == input.decision.as_str()
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
    let event = match (input.decision, cancelling) {
        (Decision::Reject, _) if comment.is_none() => return Err(ErrorCode::CommentRequired.into()),
        (Decision::Approve, false) => RequestEvent::Approve,
        (Decision::Reject, false) => RequestEvent::Reject,
        (Decision::Approve, true) => RequestEvent::AcceptCancel,
        (Decision::Reject, true) => RequestEvent::DeclineCancel,
    };
    let current = RequestStatus::parse(&r.status).unwrap_or(RequestStatus::Pending);
    let Ok(next) = transition(current, event) else {
        return Err(already_decided(&mut tx, id).await?);
    };

    lock_balance(&mut tx, r.user_id, r.leave_type_id).await?;
    if cancelling {
        let decided =
            decide_cancel(&mut tx, &state, &user, &r, next, &input, comment, now, peer).await?;
        let response = serde_json::to_value(&decided).map_err(ApiError::internal)?;
        idempotency::store(&mut tx, user.id, &key, StatusCode::OK, &response).await?;
        tx.commit().await?;
        return Ok((StatusCode::OK, Json(response)).into_response());
    }
    if !repo::apply_decision(
        &mut tx,
        id,
        input.version,
        next.as_str(),
        user.id,
        comment,
        now,
    )
    .await?
    {
        return Err(already_decided(&mut tx, id).await?);
    }
    let today = leaves_repo::org_calendar(&mut tx, user.org_id, now)
        .await?
        .today;
    match next {
        RequestStatus::Approved => {
            repo::charge(&mut tx, &r, user.id).await?;
            comp_off_repo::consume(&mut tx, r.org_id, r.user_id, id, r.days, today).await?;
        }
        _ => {
            repo::release_days(&mut tx, id).await?;
            comp_off_repo::release(&mut tx, id, Some(user.id), now, today).await?;
        }
    }
    repo::retire_action_tokens(&mut tx, id, now).await?;

    let decider_name = full_name(&mut tx, user.id).await?;
    queue_decision_emails(&mut tx, &state, &r, next, &decider_name, comment, now).await?;
    write_audit(
        &mut tx,
        &AuditEntry {
            org_id: user.org_id,
            actor_id: Some(user.id),
            action: match next {
                RequestStatus::Approved => "leave.approve",
                _ => "leave.reject",
            },
            entity_kind: "leave_request",
            entity_id: Some(id),
            before: Some(json!({ "status": r.status, "version": r.version })),
            after: Some(json!({ "status": next.as_str(), "comment": comment })),
            ip: Some(peer.ip()),
        },
    )
    .await?;

    let decided = Decided {
        status: next.as_str().into(),
        decided_by: PersonRef {
            id: user.id,
            full_name: decider_name,
        },
        decided_at: now,
    };
    let response = serde_json::to_value(&decided).map_err(ApiError::internal)?;
    idempotency::store(&mut tx, user.id, &key, StatusCode::OK, &response).await?;
    tx.commit().await?;
    Ok((StatusCode::OK, Json(response)).into_response())
}

/// Accept or decline a cancellation of started leave (FR-020). Accepting returns the days dated
/// on or after the day the cancellation was requested; declining keeps the leave approved.
#[allow(clippy::too_many_arguments)]
async fn decide_cancel(
    conn: &mut PgConnection,
    state: &AppState,
    user: &CurrentUser,
    r: &RequestRow,
    next: RequestStatus,
    input: &DecideInput,
    comment: Option<&str>,
    now: DateTime<Utc>,
    peer: SocketAddr,
) -> ApiResult<Decided> {
    if !repo::move_status(
        conn,
        r.id,
        &r.status,
        next.as_str(),
        Some(input.version),
        None,
        now,
    )
    .await?
    {
        return Err(already_decided(conn, r.id).await?);
    }
    let decider_name = full_name(conn, user.id).await?;
    let returned = if next == RequestStatus::Cancelled {
        let from = cancel::requested_on(conn, r).await?;
        let returned: Decimal = repo::reverse_from(conn, r, user.id, from)
            .await?
            .iter()
            .map(|(_, d)| d)
            .sum();
        let today = leaves_repo::org_calendar(conn, r.org_id, now).await?.today;
        comp_off_repo::give_back(conn, r.id, returned, Some(user.id), now, today).await?;
        cancel::queue_cancelled_emails(
            conn,
            state,
            r,
            returned,
            Some(&decider_name),
            r.cancel_reason.as_deref(),
            now,
        )
        .await?;
        Some(returned)
    } else {
        cancel::queue_declined_email(conn, state, r, &decider_name, comment, now).await?;
        None
    };
    write_audit(
        conn,
        &AuditEntry {
            org_id: user.org_id,
            actor_id: Some(user.id),
            action: if returned.is_some() {
                "leave.cancel_accept"
            } else {
                "leave.cancel_decline"
            },
            entity_kind: "leave_request",
            entity_id: Some(r.id),
            before: Some(json!({ "status": r.status, "version": r.version })),
            after: Some(json!({
                "status": next.as_str(), "comment": comment,
                "days_returned": returned.map(|d| d.to_string()),
            })),
            ip: Some(peer.ip()),
        },
    )
    .await?;
    Ok(Decided {
        status: next.as_str().into(),
        decided_by: PersonRef {
            id: user.id,
            full_name: decider_name,
        },
        decided_at: now,
    })
}

/// Employee: decision with comment and, on approval, a calendar invite. Notified members:
/// name, dates, days and status only (FR-059), with a neutral invite on approval.
async fn queue_decision_emails(
    conn: &mut PgConnection,
    state: &AppState,
    r: &RequestRow,
    status: RequestStatus,
    decided_by: &str,
    comment: Option<&str>,
    now: DateTime<Utc>,
) -> ApiResult<()> {
    let (requester, email, org_name): (String, String, String) = sqlx::query_as(
        "SELECT u.full_name, u.email::text, o.name FROM users u
         JOIN organizations o ON o.id = u.org_id WHERE u.id = $1",
    )
    .bind(r.user_id)
    .fetch_one(&mut *conn)
    .await?;
    let t = type_ref(conn, r.leave_type_id).await?;
    let dates = dates_text(
        r.from_date,
        r.to_date,
        core_half(&r.start_half),
        core_half(&r.end_half),
    );
    let days = days_text(r.days);
    let approved = status == RequestStatus::Approved;
    let invite = |summary: String| {
        approved
            .then(|| OutboxAttachment {
                filename: ICS_FILENAME.into(),
                content_type: ICS_CONTENT_TYPE.into(),
                content: leave_event(&r.id.to_string(), &summary, r.from_date, r.to_date, now),
            })
            .into_iter()
            .collect::<Vec<_>>()
    };
    notify(
        conn,
        &OutgoingEmail {
            org_id: Some(r.org_id),
            template: "leave_decided".into(),
            to: email,
            context: json!({
                "org_name": org_name, "base_url": state.config.base_url, "requester": requester,
                "leave_type": t.name, "dates": dates, "days_text": days,
                "status": status.as_str(), "decided_by": decided_by, "comment": comment,
            }),
            attachments: invite(format!("{} — {requester}", t.name)),
        },
        now,
    )
    .await?;
    for (to, _) in repo::notified(conn, r.id).await? {
        notify(
            conn,
            &OutgoingEmail {
                org_id: Some(r.org_id),
                template: "leave_decided_fyi".into(),
                to,
                context: json!({
                    "org_name": org_name, "requester": requester, "dates": dates,
                    "days_text": days, "status": status.as_str(),
                }),
                attachments: invite(format!("{requester} — on leave")),
            },
            now,
        )
        .await?;
    }
    Ok(())
}

#[utoipa::path(get, path = "/api/v1/approvals/action/{token}", tag = "approvals",
    params(("token" = String, Path, description = "Token from an approver email")),
    responses(
        (status = 303, description = "To `/approvals?request=…&decision=…&t=…`, or `/approvals?link=used|expired|invalid`; deciding there needs a session"),
    ))]
pub async fn action_link(
    State(state): State<AppState>,
    Path(token): Path<String>,
) -> ApiResult<Response> {
    let mut conn = state.db.acquire().await?;
    let check = action_token::check(
        &mut conn,
        state.config.secret_key.as_bytes(),
        &token,
        state.clock.now(),
    )
    .await?;
    let location = match check {
        Check::Valid(t) => format!(
            "/approvals?request={}&decision={}&t={token}",
            t.request_id, t.decision
        ),
        Check::Used => "/approvals?link=used".into(),
        Check::Expired => "/approvals?link=expired".into(),
        Check::Invalid => "/approvals?link=invalid".into(),
    };
    Ok((
        StatusCode::SEE_OTHER,
        [
            (header::LOCATION, location),
            (header::CACHE_CONTROL, "no-store".into()),
        ],
    )
        .into_response())
}
