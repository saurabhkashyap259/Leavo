//! Leave requests (T106): preview, create and detail (FR-016–FR-019, FR-022, FR-055, FR-059).
use crate::app::AppState;
use crate::auth::{action_token, CurrentUser};
use crate::error::{field_error, ApiError, ApiResult};
use crate::extract::ApiJson;
use crate::middleware::idempotency::{self, Claim, IdempotencyKey};
use axum::extract::{ConnectInfo, Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use chrono::{DateTime, NaiveDate, Utc};
use leavo_core::calendar::{leave_year_of, Excluded, ExcludedReason, Half};
use leavo_core::error::{DomainError, ErrorCode};
use leavo_core::permissions::Role;
use leavo_core::policy::{
    validate_leave_request, LeaveAssessment, LeaveRequestInput, LeaveRules, RequestContext,
    MAX_NOTIFY,
};
use leavo_db::audit::{write_audit, AuditEntry};
use leavo_db::comp_off_repo;
use leavo_db::leave_types_repo::{self, LeaveTypeRow};
use leavo_db::leaves_repo::{self, NewLeaveRequest, Notified, OrgCalendar};
use leavo_db::outbox::{notify, OutgoingEmail};
use leavo_db::tx::{begin_org_tx, lock_balance};
use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::PgConnection;
use std::net::SocketAddr;
use utoipa::ToSchema;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum DayHalf {
    First,
    Second,
}

impl From<DayHalf> for Half {
    fn from(h: DayHalf) -> Self {
        match h {
            DayHalf::First => Half::First,
            DayHalf::Second => Half::Second,
        }
    }
}

impl From<Half> for DayHalf {
    fn from(h: Half) -> Self {
        match h {
            Half::First => DayHalf::First,
            Half::Second => DayHalf::Second,
        }
    }
}

/// A notified member: a colleague by `user_id` or anyone by `email` (FR-059).
#[derive(Debug, Clone, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct NotifyInput {
    pub user_id: Option<Uuid>,
    pub email: Option<String>,
}

/// Body of `POST /leaves` and `POST /leaves/preview`.
#[derive(Debug, Clone, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct LeaveInput {
    pub leave_type_id: Uuid,
    pub from_date: NaiveDate,
    pub to_date: NaiveDate,
    #[serde(default)]
    pub start_half: Option<DayHalf>,
    #[serde(default)]
    pub end_half: Option<DayHalf>,
    /// 1–500 characters (optional while previewing).
    #[serde(default)]
    pub reason: String,
    #[serde(default)]
    pub attachment_id: Option<Uuid>,
    /// At most 20.
    #[serde(default)]
    pub notify: Vec<NotifyInput>,
}

#[derive(Serialize, ToSchema)]
pub struct ExcludedDate {
    pub date: NaiveDate,
    /// `weekend` or `holiday`.
    pub reason: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

#[derive(Serialize, ToSchema)]
pub struct YearCharge {
    pub leave_year: i32,
    pub days: f64,
    pub available_after: f64,
}

#[derive(Serialize, ToSchema)]
pub struct PreviewError {
    pub code: String,
    pub message: String,
}

#[derive(Serialize, ToSchema)]
pub struct Preview {
    pub days: f64,
    pub excluded: Vec<ExcludedDate>,
    pub by_leave_year: Vec<YearCharge>,
    /// Every rule the request breaks, in catalogue codes; empty when it can be submitted.
    pub errors: Vec<PreviewError>,
}

#[derive(Serialize, ToSchema)]
pub struct Created {
    pub id: Uuid,
    /// Always `pending`.
    pub status: String,
    pub days: f64,
    pub excluded: Vec<ExcludedDate>,
    pub by_leave_year: Vec<YearCharge>,
}

pub fn num(d: Decimal) -> f64 {
    d.to_f64().unwrap_or_default()
}

fn excluded(list: &[Excluded]) -> Vec<ExcludedDate> {
    list.iter()
        .map(|e| ExcludedDate {
            date: e.date,
            reason: match e.reason {
                ExcludedReason::Weekend => "weekend",
                ExcludedReason::Holiday => "holiday",
            }
            .into(),
            name: e.name.clone(),
        })
        .collect()
}

fn charges(a: &LeaveAssessment) -> Vec<YearCharge> {
    a.by_leave_year
        .iter()
        .map(|y| YearCharge {
            leave_year: y.leave_year,
            days: num(y.days),
            available_after: num(y.available_after),
        })
        .collect()
}

fn rules_of(t: Option<&LeaveTypeRow>, requestable: bool) -> LeaveRules {
    match t {
        Some(t) => LeaveRules {
            requestable,
            allow_half_day: t.allow_half_day,
            allow_negative: t.allow_negative,
            negative_limit: t.negative_limit,
            max_days_per_request: t.max_days_per_request,
            min_notice_days: t.min_notice_days,
            max_backdate_days: t.max_backdate_days,
            attachment_required_above_days: t.attachment_required_above_days,
        },
        None => LeaveRules {
            requestable: false,
            allow_half_day: true,
            allow_negative: false,
            negative_limit: Decimal::ZERO,
            max_days_per_request: None,
            min_notice_days: 0,
            max_backdate_days: 30,
            attachment_required_above_days: None,
        },
    }
}

struct Evaluation {
    leave_type: Option<LeaveTypeRow>,
    assessment: LeaveAssessment,
}

/// Run the engine for `input` with everything it needs from the database.
async fn evaluate(
    conn: &mut PgConnection,
    user: Uuid,
    cal: &OrgCalendar,
    input: &LeaveInput,
    reason: &str,
    has_attachment: bool,
) -> ApiResult<Evaluation> {
    let leave_type = leave_types_repo::get(conn, input.leave_type_id).await?;
    let requestable = match &leave_type {
        Some(t) => leaves_repo::requestable(conn, t, user).await?,
        None => false,
    };
    let rules = rules_of(leave_type.as_ref(), requestable);
    let (holidays, existing, available) = if input.to_date >= input.from_date {
        let first = leave_year_of(input.from_date, cal.start_month);
        let last = leave_year_of(input.to_date, cal.start_month);
        let years: Vec<i32> = (first..=last).collect();
        let available = match &leave_type {
            Some(t) => leaves_repo::availability(conn, user, t, &years, cal).await?,
            None => leavo_core::policy::Availability::PerYear(Default::default()),
        };
        (
            leaves_repo::holidays_between(conn, input.from_date, input.to_date).await?,
            leaves_repo::existing_days(conn, user, input.from_date, input.to_date).await?,
            available,
        )
    } else {
        (
            vec![],
            vec![],
            leavo_core::policy::Availability::PerYear(Default::default()),
        )
    };
    let assessment = validate_leave_request(
        &LeaveRequestInput {
            from: input.from_date,
            to: input.to_date,
            start_half: input.start_half.map(Into::into),
            end_half: input.end_half.map(Into::into),
            reason: reason.to_string(),
            has_attachment,
            notify_count: input.notify.len(),
        },
        &RequestContext {
            today: cal.today,
            weekend: cal.weekend,
            holidays: &holidays,
            leave_year_start_month: cal.start_month,
            rules: &rules,
            available: &available,
            existing: &existing,
        },
    );
    Ok(Evaluation {
        leave_type,
        assessment,
    })
}

fn parse_input(body: &serde_json::Value) -> ApiResult<LeaveInput> {
    serde_json::from_value(body.clone()).map_err(|e| {
        ApiError::new(ErrorCode::Validation).with_details(json!({ "body": e.to_string() }))
    })
}

#[utoipa::path(post, path = "/api/v1/leaves/preview", tag = "leaves", request_body = LeaveInput,
    responses(
        (status = 200, description = "Working days, excluded dates, per-leave-year charges and every rule broken (FR-016, FR-055)", body = Preview),
        (status = 401, description = "Not signed in", body = crate::openapi::ErrorBody),
        (status = 422, description = "VALIDATION: malformed body", body = crate::openapi::ErrorBody),
    ))]
pub async fn preview(
    State(state): State<AppState>,
    user: CurrentUser,
    ApiJson(input): ApiJson<LeaveInput>,
) -> ApiResult<Json<Preview>> {
    let mut tx = begin_org_tx(&state.db, user.org_id).await?;
    let cal = leaves_repo::org_calendar(&mut tx, user.org_id, state.clock.now()).await?;
    // The reason is often still empty while the employee picks dates.
    let reason = if input.reason.trim().is_empty() {
        "-"
    } else {
        input.reason.as_str()
    };
    let eval = evaluate(
        &mut tx,
        user.id,
        &cal,
        &input,
        reason,
        input.attachment_id.is_some(),
    )
    .await?;
    tx.commit().await?;
    let a = &eval.assessment;
    Ok(Json(Preview {
        days: num(a.total()),
        excluded: excluded(&a.count.excluded),
        by_leave_year: charges(a),
        errors: a
            .errors
            .iter()
            .map(|e| PreviewError {
                code: e.code.as_str().into(),
                message: e.code.default_message().into(),
            })
            .collect(),
    }))
}

/// A syntactically valid address. Public providers are fine here: notified members can be
/// anyone (unlike org domains).
fn valid_email(e: &str) -> bool {
    let Some((local, domain)) = e.rsplit_once('@') else {
        return false;
    };
    let domain_ok = matches!(
        leavo_core::domains::normalize_domain(domain),
        Ok(_) | Err(ErrorCode::PublicDomain)
    );
    !local.is_empty()
        && e.len() <= 254
        && !local.contains('@')
        && !e.chars().any(|c| c.is_whitespace() || c == ',' || c == ';')
        && domain_ok
}

/// Resolve notify entries to unique, valid addresses (colleagues must be active org members).
pub(crate) async fn resolve_notify(
    conn: &mut PgConnection,
    list: &[NotifyInput],
) -> ApiResult<Vec<Notified>> {
    let mut out: Vec<Notified> = Vec::new();
    for n in list {
        let member = match (n.user_id, n.email.as_deref()) {
            (Some(id), None) => {
                let email: Option<String> = sqlx::query_scalar(
                    "SELECT email::text FROM users WHERE id = $1 AND status = 'active'",
                )
                .bind(id)
                .fetch_optional(&mut *conn)
                .await?;
                Notified {
                    user_id: Some(id),
                    email: email.ok_or_else(|| field_error("notify"))?,
                }
            }
            (None, Some(email)) => {
                let email = email.trim().to_lowercase();
                if !valid_email(&email) {
                    return Err(field_error("notify"));
                }
                let user_id: Option<Uuid> = sqlx::query_scalar(
                    "SELECT id FROM users WHERE email = $1 AND status = 'active'",
                )
                .bind(&email)
                .fetch_optional(&mut *conn)
                .await?;
                Notified { user_id, email }
            }
            _ => return Err(field_error("notify")),
        };
        if !out.iter().any(|m| m.email == member.email) {
            out.push(member);
        }
    }
    Ok(out)
}

/// "1 Dec 2026" or "30 Nov 2026 (second half) – 2 Dec 2026 (first half)".
pub fn dates_text(
    from: NaiveDate,
    to: NaiveDate,
    start: Option<Half>,
    end: Option<Half>,
) -> String {
    let day = |d: NaiveDate, h: Option<Half>| {
        let mut s = d.format("%-d %b %Y").to_string();
        if let Some(h) = h {
            s.push_str(match h {
                Half::First => " (first half)",
                Half::Second => " (second half)",
            });
        }
        s
    };
    if from == to {
        day(from, start.or(end))
    } else {
        format!("{} – {}", day(from, start), day(to, end))
    }
}

/// "1 day", "0.5 day", "2.5 days".
pub fn days_text(days: Decimal) -> String {
    let n = days.normalize();
    if n <= Decimal::ONE {
        format!("{n} day")
    } else {
        format!("{n} days")
    }
}

struct RequestEmail<'a> {
    secret: &'a [u8],
    org_id: Uuid,
    org_name: &'a str,
    base_url: &'a str,
    requester: &'a str,
    requester_email: &'a str,
    leave_type: &'a str,
    reason: &'a str,
    dates: String,
    days: String,
    has_attachment: bool,
}

async fn queue_request_emails(
    conn: &mut PgConnection,
    e: &RequestEmail<'_>,
    request: Uuid,
    approvers: &[Uuid],
    notified: &[Notified],
    now: DateTime<Utc>,
) -> ApiResult<()> {
    let send = |template: &str, to: &str, context: serde_json::Value| OutgoingEmail {
        org_id: Some(e.org_id),
        template: template.into(),
        to: to.to_string(),
        context,
        attachments: vec![],
    };
    notify(
        conn,
        &send(
            "leave_requested_confirm",
            e.requester_email,
            json!({
                "org_name": e.org_name, "base_url": e.base_url, "requester": e.requester,
                "leave_type": e.leave_type, "reason": e.reason, "dates": e.dates,
                "days_text": e.days, "status": "pending",
            }),
        ),
        now,
    )
    .await?;
    let approver_emails: Vec<(Uuid, String)> =
        sqlx::query_as("SELECT id, email::text FROM users WHERE id = ANY($1) ORDER BY email")
            .bind(approvers)
            .fetch_all(&mut *conn)
            .await?;
    // Each approver gets their own single-use Approve and Reject links (FR-024).
    let url = |t: &str| format!("{}/api/v1/approvals/action/{t}", e.base_url);
    for (approver, to) in approver_emails {
        let approve = action_token::mint(
            conn, e.secret, e.org_id, request, "leave", approver, "approve", now,
        )
        .await?;
        let reject = action_token::mint(
            conn, e.secret, e.org_id, request, "leave", approver, "reject", now,
        )
        .await?;
        notify(
            conn,
            &send(
                "leave_requested_action",
                &to,
                json!({
                    "org_name": e.org_name, "base_url": e.base_url, "requester": e.requester,
                    "leave_type": e.leave_type, "reason": e.reason, "dates": e.dates,
                    "days_text": e.days, "has_attachment": e.has_attachment,
                    "approve_url": url(&approve),
                    "reject_url": url(&reject),
                }),
            ),
            now,
        )
        .await?;
    }
    // FYI: name, dates, days and status only (FR-059) — no type, reason or links.
    for m in notified {
        notify(
            conn,
            &send(
                "leave_requested_fyi",
                &m.email,
                json!({
                    "org_name": e.org_name, "requester": e.requester, "dates": e.dates,
                    "days_text": e.days, "status": "pending",
                }),
            ),
            now,
        )
        .await?;
    }
    Ok(())
}

fn first_error(a: &LeaveAssessment) -> Option<ApiError> {
    a.errors.first().cloned().map(|e: DomainError| e.into())
}

#[utoipa::path(post, path = "/api/v1/leaves", tag = "leaves", request_body = LeaveInput,
    params(("Idempotency-Key" = String, Header, description = "UUID; a retry with the same key returns the first response")),
    responses(
        (status = 201, description = "Request created as pending; approvers, the employee and notified members are emailed", body = Created),
        (status = 401, description = "Not signed in", body = crate::openapi::ErrorBody),
        (status = 409, description = "IDEMPOTENCY_MISMATCH", body = crate::openapi::ErrorBody),
        (status = 422, description = "The first rule broken (catalogue code, e.g. INSUFFICIENT_BALANCE with details.leave_year), NO_APPROVER_AVAILABLE, or VALIDATION (details.field: Idempotency-Key, notify, attachment_id)", body = crate::openapi::ErrorBody),
    ))]
pub async fn create(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    user: CurrentUser,
    key: IdempotencyKey,
    ApiJson(body): ApiJson<serde_json::Value>,
) -> ApiResult<Response> {
    let input = parse_input(&body)?;
    let now = state.clock.now();
    let mut tx = begin_org_tx(&state.db, user.org_id).await?;
    let hash = idempotency::body_hash(&body);
    if let Claim::Replay(status, response) = idempotency::claim(
        &mut tx,
        user.org_id,
        user.id,
        &key,
        "POST /leaves",
        &hash,
        now,
    )
    .await?
    {
        tx.commit().await?;
        return Ok((status, Json(response)).into_response());
    }

    leaves_repo::lock_requester(&mut tx, user.id).await?;
    lock_balance(&mut tx, user.id, input.leave_type_id).await?;

    if input.notify.len() <= MAX_NOTIFY {
        // Validated before the engine so a bad address is reported as such.
        resolve_notify(&mut tx, &input.notify).await?;
    }
    if let Some(att) = input.attachment_id {
        let usable: Option<bool> = sqlx::query_scalar(
            "SELECT owner_user_id = $2 AND request_id IS NULL FROM attachments WHERE id = $1",
        )
        .bind(att)
        .bind(user.id)
        .fetch_optional(&mut *tx)
        .await?;
        if usable != Some(true) {
            return Err(field_error("attachment_id"));
        }
    }
    let cal = leaves_repo::org_calendar(&mut tx, user.org_id, now).await?;
    let reason = input.reason.trim().to_string();
    let eval = evaluate(
        &mut tx,
        user.id,
        &cal,
        &input,
        &reason,
        input.attachment_id.is_some(),
    )
    .await?;
    if let Some(err) = first_error(&eval.assessment) {
        return Err(err);
    }
    let t = eval.leave_type.expect("valid request has a type");
    let notified = resolve_notify(&mut tx, &input.notify).await?;

    let (full_name, email, group, org_name): (String, String, Option<Uuid>, String) =
        sqlx::query_as(
            "SELECT u.full_name, u.email::text, u.approver_group_id, o.name
             FROM users u JOIN organizations o ON o.id = u.org_id WHERE u.id = $1",
        )
        .bind(user.id)
        .fetch_one(&mut *tx)
        .await?;
    let approvers = leaves_repo::eligible_approvers(&mut tx, group, user.id)
        .await?
        .map_err(ApiError::from)?;

    let a = &eval.assessment;
    let id = Uuid::now_v7();
    leaves_repo::insert_request(
        &mut tx,
        &NewLeaveRequest {
            id,
            org_id: user.org_id,
            user_id: user.id,
            leave_type_id: t.id,
            from: input.from_date,
            to: input.to_date,
            start_half: input.start_half.map(Into::into),
            end_half: input.end_half.map(Into::into),
            days: a.total(),
            reason: &reason,
            routed_group_id: group,
            now,
        },
        &a.count.days,
    )
    .await
    .map_err(|e| {
        if leavo_db::orgs_repo::is_unique_violation(&e) {
            ApiError::new(ErrorCode::Overlap)
        } else {
            e.into()
        }
    })?;
    // Comp-off leave holds credits, soonest expiry first, until it is decided (FR-032, FR-060).
    comp_off_repo::reserve(&mut tx, user.org_id, user.id, id, a.total(), cal.today).await?;
    leaves_repo::insert_notifications(&mut tx, user.org_id, id, "leave", &notified).await?;
    if let Some(att) = input.attachment_id {
        sqlx::query("UPDATE attachments SET request_id = $2 WHERE id = $1")
            .bind(att)
            .bind(id)
            .execute(&mut *tx)
            .await?;
    }
    if !notified.is_empty() {
        let last: Vec<serde_json::Value> = notified
            .iter()
            .map(|m| json!({ "user_id": m.user_id, "email": m.email }))
            .collect();
        sqlx::query("UPDATE users SET last_notify_list = $2 WHERE id = $1")
            .bind(user.id)
            .bind(serde_json::Value::Array(last))
            .execute(&mut *tx)
            .await?;
    }

    let dates = dates_text(
        input.from_date,
        input.to_date,
        input.start_half.map(Into::into),
        input.end_half.map(Into::into),
    );
    queue_request_emails(
        &mut tx,
        &RequestEmail {
            secret: state.config.secret_key.as_bytes(),
            org_id: user.org_id,
            org_name: &org_name,
            base_url: &state.config.base_url,
            requester: &full_name,
            requester_email: &email,
            leave_type: &t.name,
            reason: &reason,
            dates,
            days: days_text(a.total()),
            has_attachment: input.attachment_id.is_some(),
        },
        id,
        &approvers,
        &notified,
        now,
    )
    .await?;

    let created = Created {
        id,
        status: "pending".into(),
        days: num(a.total()),
        excluded: excluded(&a.count.excluded),
        by_leave_year: charges(a),
    };
    let response = serde_json::to_value(&created).map_err(ApiError::internal)?;
    write_audit(
        &mut tx,
        &AuditEntry {
            org_id: user.org_id,
            actor_id: Some(user.id),
            action: "leave.request",
            entity_kind: "leave_request",
            entity_id: Some(id),
            before: None,
            after: Some(json!({
                "leave_type": t.code, "from": input.from_date, "to": input.to_date,
                "days": num(a.total()), "reason": reason, "routed_group_id": group,
            })),
            ip: Some(peer.ip()),
        },
    )
    .await?;
    idempotency::store(&mut tx, user.id, &key, StatusCode::CREATED, &response).await?;
    tx.commit().await?;
    Ok((StatusCode::CREATED, Json(response)).into_response())
}

#[derive(Serialize, ToSchema)]
pub struct PersonRef {
    pub id: Uuid,
    pub full_name: String,
}

#[derive(Serialize, ToSchema)]
pub struct TypeRef {
    pub id: Uuid,
    pub code: String,
    pub name: String,
    pub color: String,
}

#[derive(Serialize, ToSchema)]
pub struct AttachmentRef {
    pub id: Uuid,
    pub name: String,
}

/// Full request detail, visible to the requester, its eligible approvers and admins.
#[derive(Serialize, ToSchema)]
pub struct LeaveDetail {
    pub id: Uuid,
    pub kind: String,
    pub user: PersonRef,
    pub leave_type: TypeRef,
    pub from: NaiveDate,
    pub to: NaiveDate,
    pub start_half: Option<DayHalf>,
    pub end_half: Option<DayHalf>,
    pub days: f64,
    /// pending | approved | rejected | cancelled | cancel_requested
    pub status: String,
    pub reason: String,
    pub requested_at: DateTime<Utc>,
    pub decided_by: Option<PersonRef>,
    pub decided_at: Option<DateTime<Utc>>,
    pub decision_comment: Option<String>,
    /// Names of notified colleagues and external addresses.
    pub notify: Vec<String>,
    pub attachment: Option<AttachmentRef>,
    /// Optimistic-lock version for decisions.
    pub version: i32,
}

#[derive(sqlx::FromRow)]
struct DetailRow {
    requester: Uuid,
    requester_name: String,
    routed: Option<Uuid>,
    type_id: Uuid,
    code: String,
    type_name: String,
    color: String,
    from_date: NaiveDate,
    to_date: NaiveDate,
    start_half: Option<String>,
    end_half: Option<String>,
    days: Decimal,
    status: String,
    reason: String,
    created_at: DateTime<Utc>,
    decided_by: Option<Uuid>,
    decided_name: Option<String>,
    decided_at: Option<DateTime<Utc>>,
    decision_comment: Option<String>,
    version: i32,
}

/// Can `viewer` see this request's details (FR-058)? Requester, admins, or an eligible
/// approver from the request's routed group.
pub async fn can_view_request(
    conn: &mut PgConnection,
    viewer: &CurrentUser,
    requester: Uuid,
    routed_group: Option<Uuid>,
) -> ApiResult<bool> {
    if viewer.id == requester || viewer.role == Role::Admin {
        return Ok(true);
    }
    if viewer.role != Role::Approver {
        return Ok(false);
    }
    Ok(
        leaves_repo::eligible_approvers(conn, routed_group, requester)
            .await?
            .map(|ids| ids.contains(&viewer.id))
            .unwrap_or(false),
    )
}

#[utoipa::path(get, path = "/api/v1/leaves/{id}", tag = "leaves",
    params(("id" = Uuid, Path, description = "Request id")),
    responses(
        (status = 200, description = "Request detail including the decision", body = LeaveDetail),
        (status = 401, description = "Not signed in", body = crate::openapi::ErrorBody),
        (status = 404, description = "Not found, or not visible to this user", body = crate::openapi::ErrorBody),
    ))]
pub async fn get(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<LeaveDetail>> {
    let mut tx = begin_org_tx(&state.db, user.org_id).await?;
    let row: Option<DetailRow> = sqlx::query_as(
        "SELECT r.user_id AS requester, u.full_name AS requester_name, r.routed_group_id AS routed,
                t.id AS type_id, t.code, t.name AS type_name, t.color,
                r.from_date, r.to_date, r.start_half::text AS start_half, r.end_half::text AS end_half,
                r.days, r.status::text AS status, r.reason, r.created_at, r.decided_by,
                d.full_name AS decided_name, r.decided_at, r.decision_comment, r.version
         FROM leave_requests r
         JOIN users u ON u.id = r.user_id
         JOIN leave_types t ON t.id = r.leave_type_id
         LEFT JOIN users d ON d.id = r.decided_by
         WHERE r.id = $1",
    )
    .bind(id)
    .fetch_optional(&mut *tx)
    .await?;
    let Some(DetailRow {
        requester,
        requester_name,
        routed,
        type_id,
        code,
        type_name,
        color,
        from_date: from,
        to_date: to,
        start_half,
        end_half,
        days,
        status,
        reason,
        created_at,
        decided_by,
        decided_name,
        decided_at,
        decision_comment: comment,
        version,
    }) = row
    else {
        return Err(ErrorCode::NotFound.into());
    };
    if !can_view_request(&mut tx, &user, requester, routed).await? {
        return Err(ErrorCode::NotFound.into());
    }
    let notify: Vec<String> = sqlx::query_scalar(
        "SELECT COALESCE(u.full_name, n.email::text) FROM request_notifications n
         LEFT JOIN users u ON u.id = n.user_id
         WHERE n.request_kind = 'leave' AND n.request_id = $1 ORDER BY n.email",
    )
    .bind(id)
    .fetch_all(&mut *tx)
    .await?;
    let attachment: Option<(Uuid, String)> =
        sqlx::query_as("SELECT id, file_name FROM attachments WHERE request_id = $1")
            .bind(id)
            .fetch_optional(&mut *tx)
            .await?;
    tx.commit().await?;
    let half = |h: Option<String>| match h.as_deref() {
        Some("first") => Some(DayHalf::First),
        Some("second") => Some(DayHalf::Second),
        _ => None,
    };
    Ok(Json(LeaveDetail {
        id,
        kind: "leave".into(),
        user: PersonRef {
            id: requester,
            full_name: requester_name,
        },
        leave_type: TypeRef {
            id: type_id,
            code,
            name: type_name,
            color,
        },
        from,
        to,
        start_half: half(start_half),
        end_half: half(end_half),
        days: num(days),
        status,
        reason,
        requested_at: created_at,
        decided_by: decided_by.map(|id| PersonRef {
            id,
            full_name: decided_name.unwrap_or_default(),
        }),
        decided_at,
        decision_comment: comment,
        notify,
        attachment: attachment.map(|(id, name)| AttachmentRef { id, name }),
        version,
    }))
}
