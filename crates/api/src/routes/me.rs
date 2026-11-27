//! `/me` — profile, role, organization and balances of the signed-in user (T085).
use crate::app::AppState;
use crate::auth::CurrentUser;
use crate::error::{field_error, ApiResult};
use crate::extract::ApiJson;
use crate::org_input;
use crate::routes::approvals::half;
use crate::routes::leaves::{num, AttachmentRef, DayHalf, PersonRef, TypeRef};
use axum::extract::{Query, State};
use axum::http::header;
use axum::response::{IntoResponse, Response};
use axum::Json;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use chrono::{DateTime, NaiveDate, Utc};
use leavo_core::calendar::today_in;
use leavo_db::leaves_repo::HistoryFilter;
use leavo_db::orgs_repo::is_unique_violation;
use leavo_db::tx::begin_org_tx;
use leavo_db::{leave_types_repo, leaves_repo};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sqlx::PgConnection;
use utoipa::ToSchema;
use uuid::Uuid;

#[derive(Serialize, ToSchema)]
pub struct OrgSummary {
    pub id: Uuid,
    pub name: String,
    pub timezone: String,
    pub weekend: Vec<String>,
    pub leave_year_start_month: i16,
}

/// A member notified on the user's last request, offered again on the next one.
#[derive(Serialize, Deserialize, ToSchema)]
pub struct NotifyMember {
    pub user_id: Option<Uuid>,
    pub email: String,
}

#[derive(Serialize, ToSchema)]
pub struct Me {
    pub id: Uuid,
    pub email: String,
    pub full_name: String,
    pub role: String,
    pub department: Option<String>,
    pub employee_code: Option<String>,
    pub date_of_joining: Option<NaiveDate>,
    pub org: OrgSummary,
    /// Notified members of the user's last request (suggestions for the next one).
    pub last_notify: Vec<NotifyMember>,
}

async fn load_me(conn: &mut PgConnection, user: &CurrentUser) -> ApiResult<Me> {
    #[allow(clippy::type_complexity)]
    let (email, full_name, role, department, employee_code, doj, last_notify): (
        String,
        String,
        String,
        Option<String>,
        Option<String>,
        Option<NaiveDate>,
        serde_json::Value,
    ) = sqlx::query_as(
        "SELECT email::text, full_name, role::text, department, employee_code, date_of_joining,
                last_notify_list
         FROM users WHERE id = $1",
    )
    .bind(user.id)
    .fetch_one(&mut *conn)
    .await?;
    let (name, timezone, mask, start): (String, String, i16, i16) = sqlx::query_as(
        "SELECT name, timezone, weekend_mask, leave_year_start_month FROM organizations WHERE id = $1",
    )
    .bind(user.org_id)
    .fetch_one(&mut *conn)
    .await?;
    Ok(Me {
        id: user.id,
        email,
        full_name,
        role,
        department,
        employee_code,
        date_of_joining: doj,
        last_notify: serde_json::from_value(last_notify).unwrap_or_default(),
        org: OrgSummary {
            id: user.org_id,
            name,
            timezone,
            weekend: org_input::weekend_names(mask),
            leave_year_start_month: start,
        },
    })
}

#[utoipa::path(
    get,
    path = "/api/v1/me",
    tag = "me",
    responses(
        (status = 200, description = "The signed-in user", body = Me),
        (status = 401, description = "Not signed in", body = crate::openapi::ErrorBody),
    )
)]
pub async fn get_me(State(state): State<AppState>, user: CurrentUser) -> ApiResult<Json<Me>> {
    let mut tx = begin_org_tx(&state.db, user.org_id).await?;
    let me = load_me(&mut tx, &user).await?;
    tx.commit().await?;
    Ok(Json(me))
}

/// Date of joining is not editable here: it is set at signup and afterwards only by admins (FR-064).
#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct PatchMe {
    pub full_name: Option<String>,
    /// Empty string clears it.
    pub employee_code: Option<String>,
    /// Empty string clears it.
    pub department: Option<String>,
}

fn optional_text(field: &str, v: &str, max: usize) -> ApiResult<Option<String>> {
    let v = v.trim();
    if v.chars().count() > max {
        return Err(field_error(field));
    }
    Ok((!v.is_empty()).then(|| v.to_string()))
}

#[utoipa::path(patch, path = "/api/v1/me", tag = "me", request_body = PatchMe,
    responses(
        (status = 200, description = "Updated profile", body = Me),
        (status = 401, description = "Not signed in", body = crate::openapi::ErrorBody),
        (status = 422, description = "VALIDATION (details.field); employee codes are unique in the org", body = crate::openapi::ErrorBody),
    ))]
pub async fn patch_me(
    State(state): State<AppState>,
    user: CurrentUser,
    ApiJson(p): ApiJson<PatchMe>,
) -> ApiResult<Json<Me>> {
    let full_name = p
        .full_name
        .as_deref()
        .map(|n| org_input::name("full_name", n))
        .transpose()?;
    let code = p
        .employee_code
        .as_deref()
        .map(|c| optional_text("employee_code", c, 40))
        .transpose()?;
    let dept = p
        .department
        .as_deref()
        .map(|d| optional_text("department", d, 80))
        .transpose()?;
    let mut tx = begin_org_tx(&state.db, user.org_id).await?;
    sqlx::query(
        "UPDATE users SET
             full_name = COALESCE($2, full_name),
             employee_code = CASE WHEN $3 THEN $4 ELSE employee_code END,
             department = CASE WHEN $5 THEN $6 ELSE department END
         WHERE id = $1",
    )
    .bind(user.id)
    .bind(full_name)
    .bind(code.is_some())
    .bind(code.flatten())
    .bind(dept.is_some())
    .bind(dept.flatten())
    .execute(&mut *tx)
    .await
    .map_err(|e| {
        if is_unique_violation(&e) {
            field_error("employee_code")
        } else {
            e.into()
        }
    })?;
    let me = load_me(&mut tx, &user).await?;
    tx.commit().await?;
    Ok(Json(me))
}

#[derive(Serialize, ToSchema)]
pub struct BalanceType {
    pub id: Uuid,
    pub code: String,
    pub name: String,
    pub color: String,
    pub is_comp_off: bool,
}

/// One dashboard card (data-model.md §v_ledger_balances). Days are multiples of 0.5.
#[derive(Serialize, ToSchema)]
pub struct Balance {
    pub leave_type: BalanceType,
    /// The leave year these figures belong to (comp-off spans years).
    pub leave_year: i32,
    pub allotted: f64,
    pub carried: f64,
    pub taken: f64,
    pub adjusted: f64,
    pub pending: f64,
    pub available: f64,
    /// Comp-off only: the soonest date spendable days expire (FR-032).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_expiry: Option<crate::routes::comp_offs::NextExpiry>,
}

#[derive(Deserialize)]
pub struct BalancesQuery {
    /// Leave year (the calendar year it starts in); defaults to the current one.
    pub year: Option<i32>,
}

#[utoipa::path(get, path = "/api/v1/me/balances", tag = "me",
    params(("year" = Option<i32>, Query, description = "Leave year; defaults to the current one")),
    responses(
        (status = 200, description = "A card per leave type available to the user: active all-employee types, restricted types with an active grant, and comp-off. `pending` counts days of pending requests; `available = balance − pending`; a leave year that has not started shows its projected allotment (FR-055)", body = [Balance]),
        (status = 401, description = "Not signed in", body = crate::openapi::ErrorBody),
    ))]
pub async fn balances(
    State(state): State<AppState>,
    user: CurrentUser,
    Query(q): Query<BalancesQuery>,
) -> ApiResult<Json<Vec<Balance>>> {
    let mut tx = begin_org_tx(&state.db, user.org_id).await?;
    let cal = leaves_repo::org_calendar(&mut tx, user.org_id, state.clock.now()).await?;
    let year = match q.year {
        Some(y) if (2000..=2200).contains(&y) => y,
        Some(_) => return Err(field_error("year")),
        None => cal.current_leave_year(),
    };
    let cards = balance_cards(&mut tx, user.id, year, &cal).await?;
    tx.commit().await?;
    Ok(Json(cards))
}

/// A card per leave type available to `user` for `year` (shared with the admin view).
pub(crate) async fn balance_cards(
    tx: &mut PgConnection,
    user: Uuid,
    year: i32,
    cal: &leaves_repo::OrgCalendar,
) -> ApiResult<Vec<Balance>> {
    let ids: Vec<Uuid> = sqlx::query_scalar(
        "SELECT t.id FROM leave_types t
         WHERE t.active
           AND (t.availability = 'all' OR EXISTS (
                SELECT 1 FROM leave_type_grants g
                WHERE g.leave_type_id = t.id AND g.user_id = $1 AND g.revoked_at IS NULL))
         ORDER BY t.is_comp_off, t.sort_order, t.id",
    )
    .bind(user)
    .fetch_all(&mut *tx)
    .await?;
    let mut cards = Vec::with_capacity(ids.len());
    for id in ids {
        let Some(t) = leave_types_repo::get(&mut *tx, id).await? else {
            continue;
        };
        // `v_balances` plus a future year's projected allotment (FR-055).
        let b = leaves_repo::balance_with_projection(&mut *tx, user, &t, year, cal).await?;
        let next_expiry = if t.is_comp_off {
            let credits = leavo_db::comp_off_repo::credits_of(&mut *tx, user).await?;
            crate::routes::comp_offs::next_expiry(&credits)
        } else {
            None
        };
        cards.push(Balance {
            leave_type: BalanceType {
                id: t.id,
                code: t.code,
                name: t.name,
                color: t.color,
                is_comp_off: t.is_comp_off,
            },
            leave_year: year,
            allotted: num(b.allotted),
            carried: num(b.carried),
            taken: num(b.taken),
            adjusted: num(b.adjusted),
            pending: num(b.pending),
            available: num(b.available),
            next_expiry,
        });
    }
    Ok(cards)
}

/// One row of the leave history (FR-028). The CSV export carries the same fields.
#[derive(Serialize, ToSchema)]
pub struct HistoryItem {
    pub id: Uuid,
    /// `leave` (comp-off requests join with US7).
    pub kind: String,
    pub leave_type: TypeRef,
    pub from: NaiveDate,
    pub to: NaiveDate,
    pub start_half: Option<DayHalf>,
    pub end_half: Option<DayHalf>,
    pub days: f64,
    pub status: String,
    pub requested_at: DateTime<Utc>,
    pub decided_by: Option<PersonRef>,
    pub decided_at: Option<DateTime<Utc>>,
    pub reason: String,
    pub decision_comment: Option<String>,
    /// Members by name, outside people by email.
    pub notify: Vec<String>,
    pub attachment: Option<AttachmentRef>,
}

#[derive(Serialize, ToSchema)]
pub struct HistoryPage {
    pub items: Vec<HistoryItem>,
    /// Pass as `cursor` for the next page; `null` on the last page.
    pub next_cursor: Option<String>,
}

/// Query of `/me/leaves` and `/me/leaves.csv`. Taken as text so a bad value names its field.
#[derive(Deserialize)]
pub struct HistoryQuery {
    pub year: Option<String>,
    #[serde(rename = "type")]
    pub leave_type: Option<String>,
    pub status: Option<String>,
    pub cursor: Option<String>,
    pub limit: Option<String>,
}

const STATUSES: [&str; 5] = [
    "pending",
    "approved",
    "rejected",
    "cancelled",
    "cancel_requested",
];

fn history_filter(q: &HistoryQuery) -> ApiResult<HistoryFilter<'_>> {
    let year = q
        .year
        .as_deref()
        .map(|y| {
            y.parse::<i32>()
                .ok()
                .filter(|y| (2000..=2200).contains(y))
                .ok_or_else(|| field_error("year"))
        })
        .transpose()?;
    let leave_type = q
        .leave_type
        .as_deref()
        .map(|t| t.parse::<Uuid>().map_err(|_| field_error("type")))
        .transpose()?;
    let status = q
        .status
        .as_deref()
        .map(|s| {
            STATUSES
                .contains(&s)
                .then_some(s)
                .ok_or_else(|| field_error("status"))
        })
        .transpose()?;
    Ok(HistoryFilter {
        year,
        leave_type,
        status,
        after: None,
        limit: None,
    })
}

fn encode_cursor(from: NaiveDate, id: Uuid) -> String {
    URL_SAFE_NO_PAD.encode(format!("{from}|{id}"))
}

fn decode_cursor(c: &str) -> Option<(NaiveDate, Uuid)> {
    let raw = String::from_utf8(URL_SAFE_NO_PAD.decode(c).ok()?).ok()?;
    let (from, id) = raw.split_once('|')?;
    Some((from.parse().ok()?, id.parse().ok()?))
}

async fn history_items(
    conn: &mut PgConnection,
    user: Uuid,
    f: &HistoryFilter<'_>,
) -> ApiResult<Vec<HistoryItem>> {
    let rows = leaves_repo::history(&mut *conn, user, f).await?;
    let ids: Vec<Uuid> = rows.iter().map(|r| r.id).collect();
    let mut notified = leaves_repo::notified_names(&mut *conn, &ids).await?;
    Ok(rows
        .into_iter()
        .map(|r| HistoryItem {
            id: r.id,
            kind: "leave".into(),
            leave_type: TypeRef {
                id: r.type_id,
                code: r.code,
                name: r.type_name,
                color: r.color,
            },
            from: r.from_date,
            to: r.to_date,
            start_half: half(&r.start_half),
            end_half: half(&r.end_half),
            days: num(r.days),
            status: r.status,
            requested_at: r.created_at,
            decided_by: r
                .decided_by
                .zip(r.decided_name)
                .map(|(id, full_name)| PersonRef { id, full_name }),
            decided_at: r.decided_at,
            reason: r.reason,
            decision_comment: r.decision_comment,
            notify: notified.remove(&r.id).unwrap_or_default(),
            attachment: r
                .attachment_id
                .zip(r.attachment_name)
                .map(|(id, name)| AttachmentRef { id, name }),
        })
        .collect())
}

#[utoipa::path(get, path = "/api/v1/me/leaves", tag = "me",
    params(
        ("year" = Option<i32>, Query, description = "Leave year: requests with a day in it"),
        ("type" = Option<Uuid>, Query, description = "Leave type id"),
        ("status" = Option<String>, Query, description = "pending, approved, rejected, cancelled or cancel_requested"),
        ("cursor" = Option<String>, Query, description = "`next_cursor` of the previous page"),
        ("limit" = Option<i64>, Query, description = "1–100, default 25"),
    ),
    responses(
        (status = 200, description = "The user's requests, newest first", body = HistoryPage),
        (status = 401, description = "Not signed in", body = crate::openapi::ErrorBody),
        (status = 422, description = "VALIDATION: `details.field` names the bad parameter", body = crate::openapi::ErrorBody),
    ))]
pub async fn leaves(
    State(state): State<AppState>,
    user: CurrentUser,
    Query(q): Query<HistoryQuery>,
) -> ApiResult<Json<HistoryPage>> {
    let mut f = history_filter(&q)?;
    let limit = match q.limit.as_deref() {
        None => 25,
        Some(l) => l
            .parse::<i64>()
            .ok()
            .filter(|l| (1..=100).contains(l))
            .ok_or_else(|| field_error("limit"))?,
    };
    f.after = q
        .cursor
        .as_deref()
        .map(|c| decode_cursor(c).ok_or_else(|| field_error("cursor")))
        .transpose()?;
    // One extra row tells whether another page exists.
    f.limit = Some(limit + 1);
    let mut tx = begin_org_tx(&state.db, user.org_id).await?;
    let mut items = history_items(&mut tx, user.id, &f).await?;
    tx.commit().await?;
    let next_cursor = if items.len() as i64 > limit {
        items.truncate(limit as usize);
        items.last().map(|i| encode_cursor(i.from, i.id))
    } else {
        None
    };
    Ok(Json(HistoryPage { items, next_cursor }))
}

/// "first day: second half; last day: first half", "first half" for a single half day, or
/// empty for whole days.
fn half_day_text(i: &HistoryItem) -> String {
    let name = |h: DayHalf| match h {
        DayHalf::First => "first half",
        DayHalf::Second => "second half",
    };
    if i.from == i.to {
        return i.start_half.or(i.end_half).map(name).unwrap_or("").into();
    }
    let mut parts = Vec::new();
    if let Some(h) = i.start_half {
        parts.push(format!("first day: {}", name(h)));
    }
    if let Some(h) = i.end_half {
        parts.push(format!("last day: {}", name(h)));
    }
    parts.join("; ")
}

/// One CSV field: formula-looking text is prefixed with `'` so spreadsheets show it as text,
/// and fields with separators, quotes or line breaks are quoted.
fn csv_field(v: &str) -> String {
    let v = if v.starts_with(['=', '+', '-', '@', '\t', '\r']) {
        format!("'{v}")
    } else {
        v.to_string()
    };
    if v.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", v.replace('"', "\"\""))
    } else {
        v
    }
}

#[utoipa::path(get, path = "/api/v1/me/leaves.csv", tag = "me",
    params(
        ("year" = Option<i32>, Query, description = "Leave year"),
        ("type" = Option<Uuid>, Query, description = "Leave type id"),
        ("status" = Option<String>, Query, description = "Request status"),
    ),
    responses(
        (status = 200, description = "Every matching request (contracts/csv-formats.md §7); dates in the org's time zone", content_type = "text/csv", body = String),
        (status = 401, description = "Not signed in", body = crate::openapi::ErrorBody),
        (status = 422, description = "VALIDATION: `details.field` names the bad parameter", body = crate::openapi::ErrorBody),
    ))]
pub async fn leaves_csv(
    State(state): State<AppState>,
    user: CurrentUser,
    Query(q): Query<HistoryQuery>,
) -> ApiResult<Response> {
    let f = history_filter(&q)?;
    let mut tx = begin_org_tx(&state.db, user.org_id).await?;
    let tz = leaves_repo::org_timezone(&mut tx, user.org_id).await?;
    let items = history_items(&mut tx, user.id, &f).await?;
    tx.commit().await?;
    let date = |t: DateTime<Utc>| today_in(tz, t).to_string();
    let mut out = String::from(
        "leave_type,from,to,days,half_day,status,requested_on,decided_by,decided_on,reason,approver_comment,notified\r\n",
    );
    for i in &items {
        let fields = [
            i.leave_type.name.clone(),
            i.from.to_string(),
            i.to.to_string(),
            Decimal::try_from(i.days)
                .unwrap_or_default()
                .normalize()
                .to_string(),
            half_day_text(i),
            i.status.clone(),
            date(i.requested_at),
            i.decided_by
                .as_ref()
                .map(|p| p.full_name.clone())
                .unwrap_or_default(),
            i.decided_at.map(date).unwrap_or_default(),
            i.reason.clone(),
            i.decision_comment.clone().unwrap_or_default(),
            i.notify.join("; "),
        ];
        let line: Vec<String> = fields.iter().map(|v| csv_field(v)).collect();
        out.push_str(&line.join(","));
        out.push_str("\r\n");
    }
    Ok((
        [
            (header::CONTENT_TYPE, "text/csv; charset=utf-8"),
            (
                header::CONTENT_DISPOSITION,
                "attachment; filename=\"leave-history.csv\"",
            ),
        ],
        out,
    )
        .into_response())
}
