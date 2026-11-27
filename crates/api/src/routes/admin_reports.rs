//! Leave reports (T186, FR-044): days taken in a leave year by type, employee or month.
//!
//! Counted from booked days of approved leave, leave whose cancellation is waiting, and the days
//! that stayed taken when started leave was cancelled (their rows stay active). Pending, rejected
//! and withdrawn requests have no active rows of their own and do not count.
use crate::app::AppState;
use crate::auth::CurrentUser;
use crate::error::{field_error, ApiResult};
use crate::routes::leaves::num;
use axum::extract::{Query, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use leavo_core::permissions::Action;
use leavo_db::leaves_repo;
use leavo_db::tx::begin_org_tx;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

#[derive(Serialize, ToSchema)]
pub struct ReportType {
    pub id: Uuid,
    pub code: String,
    pub name: String,
}

#[derive(Serialize, ToSchema)]
pub struct ReportRow {
    /// The type's name, the employee's name, or `YYYY-MM`.
    pub group: String,
    /// The type, employee id, or month the row is for.
    pub key: String,
    pub leave_type: ReportType,
    pub days: f64,
}

#[derive(Deserialize)]
pub struct ReportQuery {
    pub group_by: Option<String>,
    pub year: Option<String>,
}

#[derive(sqlx::FromRow)]
struct Row {
    grp: String,
    key: String,
    type_id: Uuid,
    code: String,
    name: String,
    days: Decimal,
}

async fn rows(state: &AppState, user: &CurrentUser, q: &ReportQuery) -> ApiResult<(i32, Vec<Row>)> {
    user.require(Action::ViewReports)?;
    let (group, key, order) = match q.group_by.as_deref().unwrap_or("type") {
        "type" => ("t.name", "t.code", "t.sort_order, t.code"),
        "user" => (
            "u.full_name",
            "u.id::text",
            "lower(u.full_name), u.id::text, t.sort_order, t.code",
        ),
        "month" => (
            "to_char(d.day, 'YYYY-MM')",
            "to_char(d.day, 'YYYY-MM')",
            "to_char(d.day, 'YYYY-MM'), t.sort_order, t.code",
        ),
        _ => return Err(field_error("group_by")),
    };
    let mut tx = begin_org_tx(&state.db, user.org_id).await?;
    let cal = leaves_repo::org_calendar(&mut tx, user.org_id, state.clock.now()).await?;
    let year = match q.year.as_deref() {
        None => cal.current_leave_year(),
        Some(y) => y
            .parse()
            .ok()
            .filter(|y| (2000..=2200).contains(y))
            .ok_or_else(|| field_error("year"))?,
    };
    let out: Vec<Row> = sqlx::query_as(&format!(
        "SELECT {group} AS grp, {key} AS key, t.id AS type_id, t.code, t.name,
                SUM(d.portion) AS days
         FROM leave_request_days d
         JOIN leave_requests r ON r.id = d.leave_request_id
         JOIN leave_types t ON t.id = r.leave_type_id
         JOIN users u ON u.id = d.user_id
         WHERE d.active AND d.leave_year = $1
           AND r.status IN ('approved', 'cancel_requested', 'cancelled')
         GROUP BY {group}, {key}, t.id, t.code, t.name, t.sort_order
         ORDER BY {order}"
    ))
    .bind(year)
    .fetch_all(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok((year, out))
}

#[utoipa::path(get, path = "/api/v1/admin/reports/leaves", tag = "admin",
    params(
        ("group_by" = Option<String>, Query, description = "`type` (default), `user` or `month`"),
        ("year" = Option<i32>, Query, description = "Leave year; defaults to the current one"),
    ),
    responses(
        (status = 200, description = "Days taken, one row per group and leave type", body = [ReportRow]),
        (status = 403, description = "Not an admin", body = crate::openapi::ErrorBody),
        (status = 422, description = "VALIDATION (details.field: group_by, year)", body = crate::openapi::ErrorBody),
    ))]
pub async fn leaves(
    State(state): State<AppState>,
    user: CurrentUser,
    Query(q): Query<ReportQuery>,
) -> ApiResult<Json<Vec<ReportRow>>> {
    let (_, rows) = rows(&state, &user, &q).await?;
    Ok(Json(
        rows.into_iter()
            .map(|r| ReportRow {
                group: r.grp,
                key: r.key,
                leave_type: ReportType {
                    id: r.type_id,
                    code: r.code,
                    name: r.name,
                },
                days: num(r.days),
            })
            .collect(),
    ))
}

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

#[utoipa::path(get, path = "/api/v1/admin/reports/leaves.csv", tag = "admin",
    params(
        ("group_by" = Option<String>, Query, description = "`type` (default), `user` or `month`"),
        ("year" = Option<i32>, Query, description = "Leave year; defaults to the current one"),
    ),
    responses(
        (status = 200, description = "`group,leave_type,days` (contracts/csv-formats.md §7)", content_type = "text/csv"),
        (status = 403, description = "Not an admin", body = crate::openapi::ErrorBody),
    ))]
pub async fn leaves_csv(
    State(state): State<AppState>,
    user: CurrentUser,
    Query(q): Query<ReportQuery>,
) -> ApiResult<Response> {
    let (year, rows) = rows(&state, &user, &q).await?;
    let mut out = String::from("group,leave_type,days\r\n");
    for r in rows {
        out.push_str(&format!(
            "{},{},{}\r\n",
            csv_field(&r.grp),
            csv_field(&r.code),
            r.days.normalize()
        ));
    }
    let by = q.group_by.as_deref().unwrap_or("type");
    Ok((
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, "text/csv; charset=utf-8".to_string()),
            (
                header::CONTENT_DISPOSITION,
                format!("attachment; filename=\"leave-by-{by}-{year}.csv\""),
            ),
        ],
        out,
    )
        .into_response())
}
