//! People (T182, FR-006, FR-013, FR-014, FR-021, FR-042, FR-064): list, invite one or many by
//! CSV, and edit profile, role, group and status.
use crate::app::AppState;
use crate::auth::CurrentUser;
use crate::error::{field_error, ApiError, ApiResult};
use crate::extract::ApiJson;
use axum::extract::{ConnectInfo, Multipart, Path, Query, State};
use axum::http::StatusCode;
use axum::Json;
use chrono::{DateTime, NaiveDate, Utc};
use leavo_core::error::ErrorCode;
use leavo_core::otp::{domain_of, normalize_email};
use leavo_core::permissions::Action;
use leavo_db::audit::{write_audit, AuditEntry};
use leavo_db::outbox::{notify, OutgoingEmail};
use leavo_db::tx::begin_org_tx;
use leavo_db::users_repo::{self, NewUserRow};
use leavo_db::{auth_repo, sessions};
use leavo_importers::users::{parse_invites, InviteRow};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use sqlx::PgConnection;
use std::collections::BTreeSet;
use std::net::SocketAddr;
use utoipa::ToSchema;
use uuid::Uuid;

#[derive(Serialize, ToSchema, Clone)]
pub struct GroupRef {
    pub id: Uuid,
    pub name: String,
}

/// A person as admins see them.
#[derive(Serialize, ToSchema, Clone)]
pub struct AdminUser {
    pub id: Uuid,
    pub email: String,
    pub full_name: String,
    /// `employee`, `approver` or `admin`.
    pub role: String,
    /// `invited`, `active` or `deactivated`.
    pub status: String,
    pub employee_code: Option<String>,
    pub department: Option<String>,
    pub date_of_joining: Option<NaiveDate>,
    /// The group whose approvers decide this person's requests.
    pub approver_group: Option<GroupRef>,
    pub anonymised: bool,
}

#[derive(sqlx::FromRow)]
struct UserRow {
    id: Uuid,
    email: String,
    full_name: String,
    role: String,
    status: String,
    employee_code: Option<String>,
    department: Option<String>,
    date_of_joining: Option<NaiveDate>,
    group_id: Option<Uuid>,
    group_name: Option<String>,
    anonymised: bool,
}

impl From<UserRow> for AdminUser {
    fn from(u: UserRow) -> Self {
        Self {
            id: u.id,
            email: u.email,
            full_name: u.full_name,
            role: u.role,
            status: u.status,
            employee_code: u.employee_code,
            department: u.department,
            date_of_joining: u.date_of_joining,
            approver_group: u
                .group_id
                .zip(u.group_name)
                .map(|(id, name)| GroupRef { id, name }),
            anonymised: u.anonymised,
        }
    }
}

const SELECT: &str = "SELECT u.id, u.email::text AS email, u.full_name, u.role::text AS role,
        u.status::text AS status, u.employee_code, u.department, u.date_of_joining,
        g.id AS group_id, g.name AS group_name, u.anonymised_at IS NOT NULL AS anonymised
    FROM users u LEFT JOIN approver_groups g ON g.id = u.approver_group_id";

pub(crate) async fn load(conn: &mut PgConnection, id: Uuid) -> ApiResult<AdminUser> {
    let row: Option<UserRow> = sqlx::query_as(&format!("{SELECT} WHERE u.id = $1"))
        .bind(id)
        .fetch_optional(conn)
        .await?;
    row.map(Into::into)
        .ok_or_else(|| ApiError::new(ErrorCode::NotFound))
}

#[derive(Deserialize)]
pub struct ListQuery {
    pub q: Option<String>,
    pub status: Option<String>,
    pub role: Option<String>,
    pub group: Option<String>,
    pub cursor: Option<String>,
    pub limit: Option<String>,
}

#[derive(Serialize, ToSchema)]
pub struct UserPage {
    pub items: Vec<AdminUser>,
    /// Pass back as `cursor` for the next page; null on the last.
    pub next_cursor: Option<String>,
}

#[utoipa::path(get, path = "/api/v1/admin/users", tag = "admin",
    params(
        ("q" = Option<String>, Query, description = "Name, email or employee code contains"),
        ("status" = Option<String>, Query, description = "invited, active or deactivated"),
        ("role" = Option<String>, Query, description = "employee, approver or admin"),
        ("group" = Option<Uuid>, Query, description = "Approver group the person belongs to"),
        ("cursor" = Option<String>, Query, description = "From the previous page"),
        ("limit" = Option<i64>, Query, description = "1–200, default 50"),
    ),
    responses(
        (status = 200, description = "People by name", body = UserPage),
        (status = 403, description = "Not an admin", body = crate::openapi::ErrorBody),
        (status = 422, description = "VALIDATION (details.field)", body = crate::openapi::ErrorBody),
    ))]
pub async fn list(
    State(state): State<AppState>,
    user: CurrentUser,
    Query(q): Query<ListQuery>,
) -> ApiResult<Json<UserPage>> {
    user.require(Action::ManageUsers)?;
    let status = match q.status.as_deref() {
        None => None,
        Some(s @ ("invited" | "active" | "deactivated")) => Some(s),
        Some(_) => return Err(field_error("status")),
    };
    let role = match q.role.as_deref() {
        None => None,
        Some(r @ ("employee" | "approver" | "admin")) => Some(r),
        Some(_) => return Err(field_error("role")),
    };
    let group: Option<Uuid> = match q.group.as_deref() {
        None => None,
        Some(g) => Some(g.parse().map_err(|_| field_error("group"))?),
    };
    let limit: i64 = match q.limit.as_deref() {
        None => 50,
        Some(l) => l
            .parse()
            .ok()
            .filter(|l| (1..=200).contains(l))
            .ok_or_else(|| field_error("limit"))?,
    };
    let offset: i64 = match q.cursor.as_deref() {
        None => 0,
        Some(c) => c
            .parse()
            .ok()
            .filter(|o: &i64| *o >= 0)
            .ok_or_else(|| field_error("cursor"))?,
    };
    let search =
        q.q.as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(|s| format!("%{}%", s.replace(['%', '_'], "")));
    let mut tx = begin_org_tx(&state.db, user.org_id).await?;
    let rows: Vec<UserRow> = sqlx::query_as(&format!(
        "{SELECT}
         WHERE ($1::text IS NULL OR u.full_name ILIKE $1 OR u.email::text ILIKE $1
                OR u.employee_code ILIKE $1)
           AND ($2::text IS NULL OR u.status::text = $2)
           AND ($3::text IS NULL OR u.role::text = $3)
           AND ($4::uuid IS NULL OR u.approver_group_id = $4)
         ORDER BY lower(u.full_name), u.email, u.id
         LIMIT $5 OFFSET $6"
    ))
    .bind(&search)
    .bind(status)
    .bind(role)
    .bind(group)
    .bind(limit + 1)
    .bind(offset)
    .fetch_all(&mut *tx)
    .await?;
    tx.commit().await?;
    let more = rows.len() as i64 > limit;
    Ok(Json(UserPage {
        items: rows
            .into_iter()
            .take(limit as usize)
            .map(Into::into)
            .collect(),
        next_cursor: more.then(|| (offset + limit).to_string()),
    }))
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct InviteInput {
    pub email: String,
    pub full_name: String,
    pub employee_code: Option<String>,
    pub department: Option<String>,
    pub date_of_joining: Option<NaiveDate>,
    /// `employee` (default), `approver` or `admin`.
    pub role: Option<String>,
    pub approver_group_id: Option<Uuid>,
}

/// A validated invite.
struct Invite {
    email: String,
    full_name: String,
    employee_code: Option<String>,
    department: Option<String>,
    date_of_joining: Option<NaiveDate>,
    role: String,
    group: Option<Uuid>,
}

fn opt_text(v: Option<&str>, field: &str, max: usize) -> ApiResult<Option<String>> {
    match v.map(str::trim).filter(|s| !s.is_empty()) {
        Some(s) if s.chars().count() > max => Err(field_error(field)),
        s => Ok(s.map(str::to_string)),
    }
}

async fn domain_allowed(conn: &mut PgConnection, email: &str) -> ApiResult<bool> {
    let domain = domain_of(email).unwrap_or_default();
    Ok(
        sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM org_domains WHERE domain = $1)")
            .bind(domain)
            .fetch_one(conn)
            .await?,
    )
}

async fn code_taken(conn: &mut PgConnection, code: &str, except: Option<Uuid>) -> ApiResult<bool> {
    Ok(sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM users WHERE employee_code = $1 AND id IS DISTINCT FROM $2)",
    )
    .bind(code)
    .bind(except)
    .fetch_one(conn)
    .await?)
}

async fn group_exists(conn: &mut PgConnection, id: Uuid) -> ApiResult<bool> {
    Ok(
        sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM approver_groups WHERE id = $1)")
            .bind(id)
            .fetch_one(conn)
            .await?,
    )
}

/// Create an invited user with this year's allotment and queue the invite email.
async fn create_invited(
    conn: &mut PgConnection,
    state: &AppState,
    user: &CurrentUser,
    i: &Invite,
    now: DateTime<Utc>,
) -> ApiResult<Uuid> {
    let id = users_repo::insert_user(
        conn,
        &NewUserRow {
            org_id: user.org_id,
            email: &i.email,
            full_name: &i.full_name,
            role: &i.role,
            status: "invited",
            date_of_joining: i.date_of_joining,
        },
        now,
    )
    .await?;
    sqlx::query(
        "UPDATE users SET employee_code = $2, department = $3, approver_group_id = $4 WHERE id = $1",
    )
    .bind(id)
    .bind(&i.employee_code)
    .bind(&i.department)
    .bind(i.group)
    .execute(&mut *conn)
    .await?;
    users_repo::allot_on_join(conn, user.org_id, id, i.date_of_joining, now).await?;
    let (org_name, inviter): (String, String) = sqlx::query_as(
        "SELECT o.name, u.full_name FROM organizations o, users u WHERE o.id = $1 AND u.id = $2",
    )
    .bind(user.org_id)
    .bind(user.id)
    .fetch_one(&mut *conn)
    .await?;
    notify(
        conn,
        &OutgoingEmail {
            org_id: Some(user.org_id),
            template: "invite".into(),
            to: i.email.clone(),
            context: json!({
                "org_name": org_name, "base_url": state.config.base_url,
                "full_name": i.full_name, "invited_by": inviter,
                "login_url": format!("{}/login", state.config.base_url),
            }),
            attachments: vec![],
        },
        now,
    )
    .await?;
    Ok(id)
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
            entity_kind: "user",
            entity_id: id,
            before,
            after,
            ip: Some(peer.ip()),
        },
    )
    .await?;
    Ok(())
}

#[utoipa::path(post, path = "/api/v1/admin/users", tag = "admin", request_body = InviteInput,
    responses(
        (status = 201, description = "Invited: status `invited`, this year's quotas allotted (pro-rated by joining date) and an invite email sent", body = AdminUser),
        (status = 403, description = "Not an admin", body = crate::openapi::ErrorBody),
        (status = 422, description = "DOMAIN_NOT_REGISTERED; VALIDATION (details.field: email — invalid or already a user, full_name, employee_code — taken, department, role, approver_group_id)", body = crate::openapi::ErrorBody),
    ))]
pub async fn invite(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    user: CurrentUser,
    ApiJson(body): ApiJson<Value>,
) -> ApiResult<(StatusCode, Json<AdminUser>)> {
    user.require(Action::ManageUsers)?;
    let input: InviteInput = serde_json::from_value(body).map_err(|e| {
        ApiError::new(ErrorCode::Validation).with_details(json!({ "body": e.to_string() }))
    })?;
    let email = normalize_email(&input.email).ok_or_else(|| field_error("email"))?;
    let full_name = opt_text(Some(&input.full_name), "full_name", 120)?
        .ok_or_else(|| field_error("full_name"))?;
    let role = input.role.unwrap_or_else(|| "employee".into());
    if !["employee", "approver", "admin"].contains(&role.as_str()) {
        return Err(field_error("role"));
    }
    let invite = Invite {
        email,
        full_name,
        employee_code: opt_text(input.employee_code.as_deref(), "employee_code", 40)?,
        department: opt_text(input.department.as_deref(), "department", 120)?,
        date_of_joining: input.date_of_joining,
        role,
        group: input.approver_group_id,
    };
    let now = state.clock.now();
    let mut tx = begin_org_tx(&state.db, user.org_id).await?;
    if auth_repo::find_user(&mut tx, &invite.email)
        .await?
        .is_some()
    {
        return Err(field_error("email"));
    }
    if !domain_allowed(&mut tx, &invite.email).await? {
        return Err(ErrorCode::DomainNotRegistered.into());
    }
    if let Some(c) = &invite.employee_code {
        if code_taken(&mut tx, c, None).await? {
            return Err(field_error("employee_code"));
        }
    }
    if let Some(g) = invite.group {
        if !group_exists(&mut tx, g).await? {
            return Err(field_error("approver_group_id"));
        }
    }
    let id = create_invited(&mut tx, &state, &user, &invite, now).await?;
    let created = load(&mut tx, id).await?;
    audit(
        &mut tx,
        &user,
        "user.invite",
        Some(id),
        None,
        Some(
            json!({ "email": created.email, "full_name": created.full_name, "role": created.role,
                     "approver_group_id": invite.group }),
        ),
        peer,
    )
    .await?;
    tx.commit().await?;
    Ok((StatusCode::CREATED, Json(created)))
}

#[derive(Deserialize)]
pub struct ImportQuery {
    pub dry_run: Option<bool>,
}

#[derive(Serialize, ToSchema)]
pub struct InvitePreview {
    pub row: usize,
    pub email: String,
    pub full_name: String,
    pub role: String,
    pub approver_group: Option<String>,
}

#[derive(Serialize, ToSchema)]
pub struct SkippedInvite {
    pub row: usize,
    pub email: String,
    /// `exists`: already a user of this instance.
    pub reason: String,
}

#[derive(Serialize, ToSchema)]
pub struct ImportIssue {
    pub row: usize,
    pub code: String,
    pub message: String,
}

#[derive(Serialize, ToSchema)]
pub struct InviteImportResult {
    pub dry_run: bool,
    pub to_create: Vec<InvitePreview>,
    pub skipped: Vec<SkippedInvite>,
    /// `GROUP_WILL_BE_CREATED`.
    pub warnings: Vec<ImportIssue>,
    /// `MISSING_COLUMN`, `MISSING_VALUE`, `INVALID_EMAIL`, `UNKNOWN_DOMAIN`, `INVALID_DATE`,
    /// `INVALID_ROLE`, `DUPLICATE_ROW`, `EMPLOYEE_CODE_TAKEN`.
    pub errors: Vec<ImportIssue>,
}

#[utoipa::path(post, path = "/api/v1/admin/users/import", tag = "admin",
    params(("dry_run" = Option<bool>, Query, description = "true: report only")),
    request_body(content_type = "multipart/form-data", description = "CSV `file` (contracts/csv-formats.md §1)"),
    responses(
        (status = 200, description = "Who is (or would be) invited; existing users are skipped; missing groups are created", body = InviteImportResult),
        (status = 403, description = "Not an admin", body = crate::openapi::ErrorBody),
        (status = 422, description = "IMPORT_HAS_ERRORS (apply with row errors; nothing written); VALIDATION (details.field: file)", body = crate::openapi::ErrorBody),
    ))]
pub async fn import(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    user: CurrentUser,
    Query(q): Query<ImportQuery>,
    mut form: Multipart,
) -> ApiResult<Json<InviteImportResult>> {
    user.require(Action::ManageUsers)?;
    let dry_run = q.dry_run.unwrap_or(false);
    let mut bytes = None;
    while let Some(field) = form.next_field().await.map_err(|_| field_error("file"))? {
        if field.name() == Some("file") {
            bytes = Some(field.bytes().await.map_err(|_| field_error("file"))?);
            break;
        }
    }
    let parsed = parse_invites(&bytes.ok_or_else(|| field_error("file"))?);
    let issue = |row, code: &str, message: String| ImportIssue {
        row,
        code: code.into(),
        message,
    };
    let mut errors: Vec<ImportIssue> = parsed
        .errors
        .into_iter()
        .map(|e| issue(e.row, e.code, e.message))
        .collect();
    let now = state.clock.now();
    let mut tx = begin_org_tx(&state.db, user.org_id).await?;
    let groups: Vec<(Uuid, String)> = sqlx::query_as("SELECT id, name FROM approver_groups")
        .fetch_all(&mut *tx)
        .await?;
    let mut new_groups: BTreeSet<String> = BTreeSet::new();
    let mut warnings = Vec::new();
    let mut skipped = Vec::new();
    let mut create: Vec<InviteRow> = Vec::new();
    for r in parsed.rows {
        if auth_repo::find_user(&mut tx, &r.email).await?.is_some() {
            skipped.push(SkippedInvite {
                row: r.row,
                email: r.email,
                reason: "exists".into(),
            });
            continue;
        }
        if !domain_allowed(&mut tx, &r.email).await? {
            errors.push(issue(
                r.row,
                "UNKNOWN_DOMAIN",
                "This email's domain is not registered for the organization.".into(),
            ));
            continue;
        }
        if let Some(c) = &r.employee_code {
            if code_taken(&mut tx, c, None).await? {
                errors.push(issue(
                    r.row,
                    "EMPLOYEE_CODE_TAKEN",
                    "Another person already has this employee code.".into(),
                ));
                continue;
            }
        }
        if let Some(g) = &r.approver_group {
            let known = groups.iter().any(|(_, n)| n.eq_ignore_ascii_case(g))
                || new_groups.iter().any(|n| n.eq_ignore_ascii_case(g));
            if !known {
                new_groups.insert(g.clone());
                warnings.push(issue(
                    r.row,
                    "GROUP_WILL_BE_CREATED",
                    format!("Group \"{g}\" will be created."),
                ));
            }
        }
        create.push(r);
    }
    errors.sort_by_key(|e| e.row);
    let result = InviteImportResult {
        dry_run,
        to_create: create
            .iter()
            .map(|r| InvitePreview {
                row: r.row,
                email: r.email.clone(),
                full_name: r.full_name.clone(),
                role: r.role.clone(),
                approver_group: r.approver_group.clone(),
            })
            .collect(),
        skipped,
        warnings,
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
    let mut group_ids: Vec<(Uuid, String)> = groups;
    for name in &new_groups {
        let id = Uuid::now_v7();
        sqlx::query("INSERT INTO approver_groups (id, org_id, name) VALUES ($1, $2, $3)")
            .bind(id)
            .bind(user.org_id)
            .bind(name)
            .execute(&mut *tx)
            .await?;
        group_ids.push((id, name.clone()));
    }
    for r in &create {
        let group = r.approver_group.as_ref().and_then(|g| {
            group_ids
                .iter()
                .find(|(_, n)| n.eq_ignore_ascii_case(g))
                .map(|(id, _)| *id)
        });
        create_invited(
            &mut tx,
            &state,
            &user,
            &Invite {
                email: r.email.clone(),
                full_name: r.full_name.clone(),
                employee_code: r.employee_code.clone(),
                department: r.department.clone(),
                date_of_joining: r.date_of_joining,
                role: r.role.clone(),
                group,
            },
            now,
        )
        .await?;
    }
    audit(
        &mut tx,
        &user,
        "user.import",
        None,
        None,
        Some(
            json!({ "invited": result.to_create.len(), "skipped": result.skipped.len(),
                     "groups_created": new_groups }),
        ),
        peer,
    )
    .await?;
    tx.commit().await?;
    Ok(Json(result))
}

async fn other_active_admins(conn: &mut PgConnection, except: Uuid) -> ApiResult<i64> {
    Ok(sqlx::query_scalar(
        "SELECT count(*) FROM users WHERE role = 'admin' AND status = 'active' AND id <> $1",
    )
    .bind(except)
    .fetch_one(conn)
    .await?)
}

#[utoipa::path(patch, path = "/api/v1/admin/users/{id}", tag = "admin",
    params(("id" = Uuid, Path, description = "User id")),
    request_body(content = Object, description = "Any of `full_name`, `employee_code`, `department`, `date_of_joining` (no re-allotment, FR-064), `role`, `approver_group_id` (null: none), `status` (`active` | `deactivated`)"),
    responses(
        (status = 200, description = "Updated. Deactivation ends all the person's sessions at once (FR-006); demotion to employee removes them from the groups they approve", body = AdminUser),
        (status = 403, description = "Not an admin", body = crate::openapi::ErrorBody),
        (status = 404, description = "NOT_FOUND", body = crate::openapi::ErrorBody),
        (status = 422, description = "LAST_ADMIN (FR-014); VALIDATION (details.field)", body = crate::openapi::ErrorBody),
    ))]
pub async fn update(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    user: CurrentUser,
    Path(id): Path<Uuid>,
    ApiJson(body): ApiJson<Value>,
) -> ApiResult<Json<AdminUser>> {
    user.require(Action::ManageUsers)?;
    let obj: Map<String, Value> = body
        .as_object()
        .cloned()
        .ok_or_else(|| field_error("body"))?;
    const FIELDS: [&str; 7] = [
        "full_name",
        "employee_code",
        "department",
        "date_of_joining",
        "role",
        "approver_group_id",
        "status",
    ];
    if let Some(k) = obj.keys().find(|k| !FIELDS.contains(&k.as_str())) {
        return Err(field_error(k));
    }
    let text = |k: &str| -> ApiResult<Option<Option<String>>> {
        match obj.get(k) {
            None => Ok(None),
            Some(Value::Null) => Ok(Some(None)),
            Some(Value::String(s)) => Ok(Some(opt_text(Some(s), k, 120)?)),
            Some(_) => Err(field_error(k)),
        }
    };
    let now = state.clock.now();
    let mut tx = begin_org_tx(&state.db, user.org_id).await?;
    let before = load(&mut tx, id).await?;
    let mut after = before.clone();
    if let Some(v) = text("full_name")? {
        after.full_name = v.ok_or_else(|| field_error("full_name"))?;
    }
    if let Some(v) = text("employee_code")? {
        if let Some(c) = &v {
            if c.chars().count() > 40 || code_taken(&mut tx, c, Some(id)).await? {
                return Err(field_error("employee_code"));
            }
        }
        after.employee_code = v;
    }
    if let Some(v) = text("department")? {
        after.department = v;
    }
    if let Some(v) = obj.get("date_of_joining") {
        after.date_of_joining = match v {
            Value::Null => None,
            Value::String(s) => Some(s.parse().map_err(|_| field_error("date_of_joining"))?),
            _ => return Err(field_error("date_of_joining")),
        };
    }
    if let Some(v) = obj.get("role") {
        after.role = match v.as_str() {
            Some(r @ ("employee" | "approver" | "admin")) => r.into(),
            _ => return Err(field_error("role")),
        };
    }
    if let Some(v) = obj.get("approver_group_id") {
        let group: Option<Uuid> = match v {
            Value::Null => None,
            Value::String(s) => Some(s.parse().map_err(|_| field_error("approver_group_id"))?),
            _ => return Err(field_error("approver_group_id")),
        };
        if let Some(g) = group {
            if !group_exists(&mut tx, g).await? {
                return Err(field_error("approver_group_id"));
            }
        }
        after.approver_group = group.map(|id| GroupRef {
            id,
            name: String::new(),
        });
    }
    if let Some(v) = obj.get("status") {
        after.status = match v.as_str() {
            Some(s @ ("active" | "deactivated")) => s.into(),
            _ => return Err(field_error("status")),
        };
    }
    let was_admin = before.role == "admin" && before.status == "active";
    let stays_admin = after.role == "admin" && after.status == "active";
    if was_admin && !stays_admin && other_active_admins(&mut tx, id).await? == 0 {
        return Err(ErrorCode::LastAdmin.into());
    }
    // A person who was deactivated and is activated again: invited people stay invited until
    // they sign in.
    let status = if after.status == "active" && before.status == "invited" {
        "invited".to_string()
    } else {
        after.status.clone()
    };
    sqlx::query(
        "UPDATE users SET full_name = $2, employee_code = $3, department = $4,
                date_of_joining = $5, role = $6::user_role, approver_group_id = $7,
                status = $8::user_status
         WHERE id = $1",
    )
    .bind(id)
    .bind(&after.full_name)
    .bind(&after.employee_code)
    .bind(&after.department)
    .bind(after.date_of_joining)
    .bind(&after.role)
    .bind(after.approver_group.as_ref().map(|g| g.id))
    .bind(&status)
    .execute(&mut *tx)
    .await?;
    if after.role == "employee" && before.role != "employee" {
        sqlx::query("DELETE FROM approver_group_members WHERE user_id = $1")
            .bind(id)
            .execute(&mut *tx)
            .await?;
    }
    if after.status == "deactivated" && before.status != "deactivated" {
        sessions::revoke_all_for_user(&mut *tx, id, now).await?;
    }
    let updated = load(&mut tx, id).await?;
    let snapshot = |u: &AdminUser| {
        json!({
            "full_name": u.full_name, "employee_code": u.employee_code, "department": u.department,
            "date_of_joining": u.date_of_joining, "role": u.role, "status": u.status,
            "approver_group_id": u.approver_group.as_ref().map(|g| g.id),
        })
    };
    audit(
        &mut tx,
        &user,
        "user.update",
        Some(id),
        Some(snapshot(&before)),
        Some(snapshot(&updated)),
        peer,
    )
    .await?;
    tx.commit().await?;
    Ok(Json(updated))
}

#[utoipa::path(get, path = "/api/v1/admin/users/{id}", tag = "admin",
    params(("id" = Uuid, Path, description = "User id")),
    responses(
        (status = 200, description = "One person", body = AdminUser),
        (status = 403, description = "Not an admin", body = crate::openapi::ErrorBody),
        (status = 404, description = "NOT_FOUND", body = crate::openapi::ErrorBody),
    ))]
pub async fn get(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<AdminUser>> {
    user.require(Action::ManageUsers)?;
    let mut tx = begin_org_tx(&state.db, user.org_id).await?;
    let found = load(&mut tx, id).await?;
    tx.commit().await?;
    Ok(Json(found))
}
