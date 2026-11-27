//! Holidays (T163, FR-033–FR-035): the calendar for everyone, admin add/edit/delete, and
//! imports from CSV, a country pack or last year's list. Every change that turns a date into a
//! mandatory holiday, or back into a working day, adjusts the leave booked on it.
use crate::app::AppState;
use crate::auth::CurrentUser;
use crate::error::{field_error, ApiError, ApiResult};
use crate::extract::ApiJson;
use crate::holiday_packs::{self, PackInfo};
use crate::routes::leaves::{dates_text, days_text};
use axum::extract::{ConnectInfo, FromRequest, Multipart, Path, Query, Request, State};
use axum::http::{header, StatusCode};
use axum::Json;
use chrono::{Datelike, NaiveDate};
use leavo_core::calendar::{leave_year_bounds, HolidayKind};
use leavo_core::error::ErrorCode;
use leavo_core::permissions::{Action, Role};
use leavo_db::audit::{write_audit, AuditEntry};
use leavo_db::holidays_repo::{self as repo, Affected, HolidayRow, NewHoliday};
use leavo_db::leaves_repo;
use leavo_db::outbox::{notify, OutgoingEmail};
use leavo_db::tx::begin_org_tx;
use leavo_importers::holidays::parse_holidays;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::PgConnection;
use std::collections::BTreeSet;
use std::net::SocketAddr;
use utoipa::ToSchema;
use uuid::Uuid;

#[utoipa::path(get, path = "/api/v1/holiday-packs", tag = "admin",
    responses((status = 200, description = "Country holiday packs bundled with this release", body = [PackInfo]),
              (status = 403, description = "Not an admin", body = crate::openapi::ErrorBody)))]
pub async fn packs(
    State(_state): State<AppState>,
    user: CurrentUser,
) -> ApiResult<Json<Vec<PackInfo>>> {
    user.require(Action::ManageHolidays)?;
    Ok(Json(holiday_packs::list()))
}

/// One holiday of the organization's calendar.
#[derive(Serialize, ToSchema)]
pub struct HolidayItem {
    pub id: Uuid,
    pub date: NaiveDate,
    pub name: String,
    /// `mandatory` or `optional`.
    pub kind: String,
    pub location: Option<String>,
}

impl From<HolidayRow> for HolidayItem {
    fn from(h: HolidayRow) -> Self {
        Self {
            id: h.id,
            date: h.date,
            name: h.name,
            kind: h.kind,
            location: h.location,
        }
    }
}

#[derive(Deserialize)]
pub struct HolidaysQuery {
    pub year: Option<String>,
}

#[utoipa::path(get, path = "/api/v1/holidays", tag = "me",
    params(("year" = Option<i32>, Query, description = "Leave year; the current (default) or next one, any year for admins")),
    responses(
        (status = 200, description = "The leave year's holidays by date (FR-033)", body = [HolidayItem]),
        (status = 401, description = "Not signed in", body = crate::openapi::ErrorBody),
        (status = 422, description = "VALIDATION: `year` is not the current or next leave year (admins: 2000–2200)", body = crate::openapi::ErrorBody),
    ))]
pub async fn list(
    State(state): State<AppState>,
    user: CurrentUser,
    Query(q): Query<HolidaysQuery>,
) -> ApiResult<Json<Vec<HolidayItem>>> {
    let mut tx = begin_org_tx(&state.db, user.org_id).await?;
    let cal = leaves_repo::org_calendar(&mut tx, user.org_id, state.clock.now()).await?;
    let current = cal.current_leave_year();
    let admin = user.role == Role::Admin;
    let year = match q.year.as_deref() {
        None => current,
        Some(y) => y
            .parse::<i32>()
            .ok()
            .filter(|y| {
                if admin {
                    (2000..=2200).contains(y)
                } else {
                    *y == current || *y == current + 1
                }
            })
            .ok_or_else(|| field_error("year"))?,
    };
    let (from, to) = leave_year_bounds(year, cal.start_month);
    let rows = repo::between(&mut tx, from, to).await?;
    tx.commit().await?;
    Ok(Json(rows.into_iter().map(Into::into).collect()))
}

fn kind_of(s: &str) -> Option<&'static str> {
    match s {
        "mandatory" => Some("mandatory"),
        "optional" => Some("optional"),
        _ => None,
    }
}

fn clean_name(name: &str) -> ApiResult<String> {
    let n = name.trim();
    if n.is_empty() || n.chars().count() > 120 {
        return Err(field_error("name"));
    }
    Ok(n.to_string())
}

fn clean_location(location: Option<&str>) -> ApiResult<Option<String>> {
    match location.map(str::trim).filter(|l| !l.is_empty()) {
        Some(l) if l.chars().count() > 120 => Err(field_error("location")),
        l => Ok(l.map(str::to_string)),
    }
}

/// Email what a holiday change did to approved leave (FR-035): the employee always; admins too
/// when a day was charged again. Pending requests are only recounted.
async fn queue_change_emails(
    conn: &mut PgConnection,
    state: &AppState,
    org: Uuid,
    affected: &[Affected],
    now: chrono::DateTime<chrono::Utc>,
) -> ApiResult<()> {
    if affected.iter().all(|a| a.status == "pending") {
        return Ok(());
    }
    let org_name: String = sqlx::query_scalar("SELECT name FROM organizations WHERE id = $1")
        .bind(org)
        .fetch_one(&mut *conn)
        .await?;
    let admins: Vec<String> = sqlx::query_scalar(
        "SELECT email::text FROM users WHERE role = 'admin' AND status = 'active' ORDER BY email",
    )
    .fetch_all(&mut *conn)
    .await?;
    for a in affected.iter().filter(|a| a.status != "pending") {
        let (name, email, type_name, from, to, sh, eh): (
            String,
            String,
            String,
            NaiveDate,
            NaiveDate,
            Option<String>,
            Option<String>,
        ) = sqlx::query_as(
            "SELECT u.full_name, u.email::text, t.name, r.from_date, r.to_date,
                    r.start_half::text, r.end_half::text
             FROM leave_requests r JOIN users u ON u.id = r.user_id
             JOIN leave_types t ON t.id = r.leave_type_id WHERE r.id = $1",
        )
        .bind(a.request)
        .fetch_one(&mut *conn)
        .await?;
        let booked = a.delta < Decimal::ZERO;
        let context = |to_requester: bool| {
            json!({
                "org_name": org_name, "base_url": state.config.base_url, "requester": name,
                "leave_type": type_name,
                "dates": dates_text(
                    from,
                    to,
                    crate::routes::approvals::core_half(&sh),
                    crate::routes::approvals::core_half(&eh),
                ),
                "holiday_date": a.day.format("%a %-d %b %Y").to_string(),
                "days_text": days_text(a.delta.abs()),
                "booked": booked, "to_requester": to_requester,
            })
        };
        let mut recipients = vec![(email, context(true))];
        if booked {
            recipients.extend(admins.iter().map(|to| (to.clone(), context(false))));
        }
        for (to, context) in recipients {
            notify(
                conn,
                &OutgoingEmail {
                    org_id: Some(org),
                    template: "holiday_changed_leave".into(),
                    to,
                    context,
                    attachments: vec![],
                },
                now,
            )
            .await?;
        }
    }
    Ok(())
}

/// Run `change` and then apply its effect on booked leave for `dates` (FR-035).
async fn with_side_effects<'c, F, T>(
    conn: &'c mut PgConnection,
    state: &AppState,
    user: &CurrentUser,
    dates: &[NaiveDate],
    change: F,
) -> ApiResult<(T, Vec<Affected>)>
where
    F: for<'a> FnOnce(
        &'a mut PgConnection,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = ApiResult<T>> + Send + 'a>,
    >,
{
    let now = state.clock.now();
    let before = repo::mandatory_dates(conn, dates).await?;
    let out = change(conn).await?;
    let affected =
        repo::apply_holiday_change(conn, user.org_id, user.id, dates, &before, now).await?;
    queue_change_emails(conn, state, user.org_id, &affected, now).await?;
    Ok((out, affected))
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct HolidayInput {
    pub date: NaiveDate,
    /// 1–120 characters.
    pub name: String,
    /// `mandatory` (default) or `optional`.
    pub kind: Option<String>,
    pub location: Option<String>,
}

fn parse<T: serde::de::DeserializeOwned>(body: Value) -> ApiResult<T> {
    serde_json::from_value(body).map_err(|e| {
        ApiError::new(ErrorCode::Validation).with_details(json!({ "body": e.to_string() }))
    })
}

async fn audit(
    conn: &mut PgConnection,
    user: &CurrentUser,
    action: &'static str,
    id: Option<Uuid>,
    before: Option<Value>,
    after: Option<Value>,
    peer: SocketAddr,
) -> ApiResult<()> {
    write_audit(
        conn,
        &AuditEntry {
            org_id: user.org_id,
            actor_id: Some(user.id),
            action,
            entity_kind: "holiday",
            entity_id: id,
            before,
            after,
            ip: Some(peer.ip()),
        },
    )
    .await?;
    Ok(())
}

#[utoipa::path(post, path = "/api/v1/admin/holidays", tag = "admin", request_body = HolidayInput,
    responses(
        (status = 201, description = "Added. A mandatory holiday frees that day in pending and approved leave (FR-035)", body = HolidayItem),
        (status = 403, description = "Not an admin", body = crate::openapi::ErrorBody),
        (status = 422, description = "VALIDATION (details.field: name — empty, too long or the same date and name exists; kind; location)", body = crate::openapi::ErrorBody),
    ))]
pub async fn create(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    user: CurrentUser,
    ApiJson(body): ApiJson<Value>,
) -> ApiResult<(StatusCode, Json<HolidayItem>)> {
    user.require(Action::ManageHolidays)?;
    let input: HolidayInput = parse(body)?;
    let name = clean_name(&input.name)?;
    let kind =
        kind_of(input.kind.as_deref().unwrap_or("mandatory")).ok_or_else(|| field_error("kind"))?;
    let location = clean_location(input.location.as_deref())?;
    let mut tx = begin_org_tx(&state.db, user.org_id).await?;
    if repo::exists(&mut tx, input.date, &name, None).await? {
        return Err(field_error("name"));
    }
    let org = user.org_id;
    let (row, affected) = with_side_effects(&mut tx, &state, &user, &[input.date], |c| {
        let (name, location) = (name.clone(), location.clone());
        Box::pin(async move {
            Ok(repo::insert(
                c,
                org,
                &NewHoliday {
                    date: input.date,
                    name: &name,
                    kind,
                    location: location.as_deref(),
                    source: "manual",
                },
            )
            .await?)
        })
    })
    .await?;
    audit(
        &mut tx,
        &user,
        "holiday.create",
        Some(row.id),
        None,
        Some(json!({ "date": row.date, "name": row.name, "kind": row.kind, "leave_changed": affected.len() })),
        peer,
    )
    .await?;
    tx.commit().await?;
    Ok((StatusCode::CREATED, Json(row.into())))
}

#[utoipa::path(patch, path = "/api/v1/admin/holidays/{id}", tag = "admin",
    params(("id" = Uuid, Path, description = "Holiday id")),
    request_body(content = Object, description = "Any of `date`, `name`, `kind`, `location` (null clears it)"),
    responses(
        (status = 200, description = "Updated; booked leave on the old and new date is adjusted (FR-035)", body = HolidayItem),
        (status = 403, description = "Not an admin", body = crate::openapi::ErrorBody),
        (status = 404, description = "NOT_FOUND", body = crate::openapi::ErrorBody),
        (status = 422, description = "VALIDATION (details.field: date, name, kind, location)", body = crate::openapi::ErrorBody),
    ))]
pub async fn update(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    user: CurrentUser,
    Path(id): Path<Uuid>,
    ApiJson(body): ApiJson<Value>,
) -> ApiResult<Json<HolidayItem>> {
    user.require(Action::ManageHolidays)?;
    let obj = body.as_object().ok_or_else(|| field_error("body"))?.clone();
    if let Some(k) = obj
        .keys()
        .find(|k| !["date", "name", "kind", "location"].contains(&k.as_str()))
    {
        return Err(field_error(k));
    }
    let mut tx = begin_org_tx(&state.db, user.org_id).await?;
    let old = repo::get(&mut tx, id)
        .await?
        .ok_or_else(|| ApiError::new(ErrorCode::NotFound))?;
    let mut new = old.clone();
    if let Some(v) = obj.get("date") {
        new.date = v
            .as_str()
            .and_then(|s| s.parse().ok())
            .ok_or_else(|| field_error("date"))?;
    }
    if let Some(v) = obj.get("name") {
        new.name = clean_name(v.as_str().ok_or_else(|| field_error("name"))?)?;
    }
    if let Some(v) = obj.get("kind") {
        new.kind = v
            .as_str()
            .and_then(kind_of)
            .ok_or_else(|| field_error("kind"))?
            .to_string();
    }
    if let Some(v) = obj.get("location") {
        new.location = match v {
            Value::Null => None,
            Value::String(s) => clean_location(Some(s))?,
            _ => return Err(field_error("location")),
        };
    }
    if repo::exists(&mut tx, new.date, &new.name, Some(id)).await? {
        return Err(field_error("name"));
    }
    let row = new.clone();
    let (updated, affected) =
        with_side_effects(&mut tx, &state, &user, &[old.date, new.date], |c| {
            Box::pin(async move { Ok(repo::update(c, &row).await?) })
        })
        .await?;
    audit(
        &mut tx,
        &user,
        "holiday.update",
        Some(id),
        Some(json!({ "date": old.date, "name": old.name, "kind": old.kind, "location": old.location })),
        Some(json!({ "date": updated.date, "name": updated.name, "kind": updated.kind,
                     "location": updated.location, "leave_changed": affected.len() })),
        peer,
    )
    .await?;
    tx.commit().await?;
    Ok(Json(updated.into()))
}

#[utoipa::path(delete, path = "/api/v1/admin/holidays/{id}", tag = "admin",
    params(("id" = Uuid, Path, description = "Holiday id")),
    responses(
        (status = 204, description = "Deleted. A mandatory holiday inside approved leave is charged again, even below zero, and the employee and admins are emailed (FR-035)"),
        (status = 403, description = "Not an admin", body = crate::openapi::ErrorBody),
        (status = 404, description = "NOT_FOUND", body = crate::openapi::ErrorBody),
    ))]
pub async fn remove(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    user: CurrentUser,
    Path(id): Path<Uuid>,
) -> ApiResult<StatusCode> {
    user.require(Action::ManageHolidays)?;
    let mut tx = begin_org_tx(&state.db, user.org_id).await?;
    let old = repo::get(&mut tx, id)
        .await?
        .ok_or_else(|| ApiError::new(ErrorCode::NotFound))?;
    let ((), affected) = with_side_effects(&mut tx, &state, &user, &[old.date], |c| {
        Box::pin(async move { Ok(repo::delete(c, id).await?) })
    })
    .await?;
    audit(
        &mut tx,
        &user,
        "holiday.delete",
        Some(id),
        Some(json!({ "date": old.date, "name": old.name, "kind": old.kind })),
        Some(json!({ "leave_changed": affected.len() })),
        peer,
    )
    .await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
pub struct ImportQuery {
    pub dry_run: Option<bool>,
}

/// JSON body of an import: a bundled pack, or a copy of another year's list.
#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ImportSource {
    /// e.g. `IN/2027` (see `/holiday-packs`).
    pub pack: Option<String>,
    pub copy_from_year: Option<i32>,
    pub to_year: Option<i32>,
}

#[derive(Serialize, ToSchema, Clone)]
pub struct ImportedHoliday {
    pub date: NaiveDate,
    pub name: String,
    pub kind: String,
    pub location: Option<String>,
}

#[derive(Serialize, ToSchema)]
pub struct SkippedHoliday {
    pub date: NaiveDate,
    pub name: String,
    /// `exists`: the same date and name is already in the calendar.
    pub reason: String,
}

#[derive(Serialize, ToSchema)]
pub struct ImportError {
    /// 1-based, counting the header line.
    pub row: usize,
    /// `MISSING_COLUMN`, `MISSING_VALUE`, `INVALID_DATE`, `INVALID_KIND` or `DUPLICATE_ROW`.
    pub code: String,
    pub message: String,
}

#[derive(Serialize, ToSchema)]
pub struct ImportResult {
    pub dry_run: bool,
    pub added: Vec<ImportedHoliday>,
    pub skipped: Vec<SkippedHoliday>,
    pub errors: Vec<ImportError>,
}

/// The same month and day in another year (29 Feb becomes 28 Feb).
fn shift_year(d: NaiveDate, year: i32) -> NaiveDate {
    d.with_year(year)
        .or_else(|| NaiveDate::from_ymd_opt(year, d.month(), 28))
        .unwrap_or(d)
}

fn kind_text(k: HolidayKind) -> String {
    match k {
        HolidayKind::Mandatory => "mandatory".into(),
        HolidayKind::Optional => "optional".into(),
    }
}

#[utoipa::path(post, path = "/api/v1/admin/holidays/import", tag = "admin",
    params(("dry_run" = Option<bool>, Query, description = "true: report what would change and write nothing")),
    request_body(content = ImportSource, description = "JSON `{ pack }` or `{ copy_from_year, to_year }`, or multipart/form-data with a CSV `file` (contracts/csv-formats.md §6)"),
    responses(
        (status = 200, description = "What was (or would be) added; rows already in the calendar are skipped. Added mandatory holidays adjust booked leave (FR-035)", body = ImportResult),
        (status = 403, description = "Not an admin", body = crate::openapi::ErrorBody),
        (status = 422, description = "IMPORT_HAS_ERRORS (apply with row errors; nothing written); VALIDATION (details.field: file, pack, copy_from_year, to_year)", body = crate::openapi::ErrorBody),
    ))]
pub async fn import(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    user: CurrentUser,
    Query(q): Query<ImportQuery>,
    req: Request,
) -> ApiResult<Json<ImportResult>> {
    user.require(Action::ManageHolidays)?;
    let dry_run = q.dry_run.unwrap_or(false);
    let multipart = req
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.starts_with("multipart/form-data"));
    let mut tx = begin_org_tx(&state.db, user.org_id).await?;
    let mut candidates: Vec<ImportedHoliday> = Vec::new();
    let mut errors: Vec<ImportError> = Vec::new();
    let source: String;
    if multipart {
        let mut form = Multipart::from_request(req, &state)
            .await
            .map_err(|_| field_error("file"))?;
        let mut bytes = None;
        while let Some(field) = form.next_field().await.map_err(|_| field_error("file"))? {
            if field.name() == Some("file") {
                bytes = Some(field.bytes().await.map_err(|_| field_error("file"))?);
                break;
            }
        }
        let bytes = bytes.ok_or_else(|| field_error("file"))?;
        let parsed = parse_holidays(&bytes);
        errors.extend(parsed.errors.into_iter().map(|e| ImportError {
            row: e.row,
            code: e.code.into(),
            message: e.message,
        }));
        candidates.extend(parsed.rows.into_iter().map(|r| ImportedHoliday {
            date: r.date,
            name: r.name,
            kind: kind_text(r.kind),
            location: r.location,
        }));
        source = "csv".into();
    } else {
        let ApiJson(body) = ApiJson::<Value>::from_request(req, &state).await?;
        let src: ImportSource = parse(body)?;
        match (src.pack, src.copy_from_year, src.to_year) {
            (Some(pack), None, None) => {
                let rows = holiday_packs::get(&pack).ok_or_else(|| field_error("pack"))?;
                candidates.extend(rows.into_iter().map(|r| ImportedHoliday {
                    date: r.date,
                    name: r.name,
                    kind: r.kind,
                    location: None,
                }));
                source = format!("pack:{pack}");
            }
            (None, Some(from), Some(to)) => {
                if !(2000..=2200).contains(&from) {
                    return Err(field_error("copy_from_year"));
                }
                if !(2000..=2200).contains(&to) || to == from {
                    return Err(field_error("to_year"));
                }
                let cal =
                    leaves_repo::org_calendar(&mut tx, user.org_id, state.clock.now()).await?;
                let (start, end) = leave_year_bounds(from, cal.start_month);
                for h in repo::between(&mut tx, start, end).await? {
                    candidates.push(ImportedHoliday {
                        date: shift_year(h.date, h.date.year() + (to - from)),
                        name: h.name,
                        kind: h.kind,
                        location: h.location,
                    });
                }
                source = format!("copy:{from}");
            }
            _ => return Err(field_error("pack")),
        }
    }

    let mut added = Vec::new();
    let mut skipped = Vec::new();
    let mut seen = BTreeSet::new();
    for c in candidates {
        let key = (c.date, c.name.to_lowercase());
        if !seen.insert(key) || repo::exists(&mut tx, c.date, &c.name, None).await? {
            skipped.push(SkippedHoliday {
                date: c.date,
                name: c.name,
                reason: "exists".into(),
            });
        } else {
            added.push(c);
        }
    }
    added.sort_by(|a, b| (a.date, &a.name).cmp(&(b.date, &b.name)));
    let result = ImportResult {
        dry_run,
        added,
        skipped,
        errors,
    };
    if dry_run {
        tx.commit().await?;
        return Ok(Json(result));
    }
    if !result.errors.is_empty() {
        return Err(ApiError::new(ErrorCode::ImportHasErrors)
            .with_details(json!({ "errors": result.errors.len() })));
    }
    let dates: Vec<NaiveDate> = result.added.iter().map(|a| a.date).collect();
    let org = user.org_id;
    let rows = result.added.clone();
    let src = source.clone();
    let ((), affected) = with_side_effects(&mut tx, &state, &user, &dates, |c| {
        Box::pin(async move {
            for r in &rows {
                repo::insert(
                    c,
                    org,
                    &NewHoliday {
                        date: r.date,
                        name: &r.name,
                        kind: &r.kind,
                        location: r.location.as_deref(),
                        source: &src,
                    },
                )
                .await?;
            }
            Ok(())
        })
    })
    .await?;
    audit(
        &mut tx,
        &user,
        "holiday.import",
        None,
        None,
        Some(json!({ "source": source, "added": result.added.len(),
                     "skipped": result.skipped.len(), "leave_changed": affected.len() })),
        peer,
    )
    .await?;
    tx.commit().await?;
    Ok(Json(result))
}
