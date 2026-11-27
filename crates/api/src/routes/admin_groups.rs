//! Approver groups (T183, FR-021): a group's members (approvers or admins) decide the requests
//! of the employees assigned to it. Each employee belongs to at most one group
//! (`users.approver_group_id`, set via `PATCH /admin/users/{id}`).
use crate::app::AppState;
use crate::auth::CurrentUser;
use crate::error::{field_error, ApiError, ApiResult};
use crate::extract::ApiJson;
use axum::extract::{ConnectInfo, Path, State};
use axum::http::StatusCode;
use axum::Json;
use leavo_core::error::ErrorCode;
use leavo_core::permissions::Action;
use leavo_db::audit::{write_audit, AuditEntry};
use leavo_db::tx::begin_org_tx;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::PgConnection;
use std::net::SocketAddr;
use utoipa::ToSchema;
use uuid::Uuid;

#[derive(Serialize, ToSchema)]
pub struct GroupMember {
    pub id: Uuid,
    pub full_name: String,
    pub email: String,
    pub role: String,
}

#[derive(Serialize, ToSchema)]
pub struct ApproverGroup {
    pub id: Uuid,
    pub name: String,
    /// Approvers and admins who decide this group's requests.
    pub members: Vec<GroupMember>,
    /// People assigned to the group.
    pub employees: i64,
}

async fn load(conn: &mut PgConnection, id: Uuid) -> ApiResult<ApproverGroup> {
    let (name,): (String,) = sqlx::query_as("SELECT name FROM approver_groups WHERE id = $1")
        .bind(id)
        .fetch_optional(&mut *conn)
        .await?
        .ok_or_else(|| ApiError::new(ErrorCode::NotFound))?;
    let members: Vec<(Uuid, String, String, String)> = sqlx::query_as(
        "SELECT u.id, u.full_name, u.email::text, u.role::text FROM approver_group_members m
         JOIN users u ON u.id = m.user_id WHERE m.group_id = $1 ORDER BY lower(u.full_name), u.id",
    )
    .bind(id)
    .fetch_all(&mut *conn)
    .await?;
    let employees: i64 =
        sqlx::query_scalar("SELECT count(*) FROM users WHERE approver_group_id = $1")
            .bind(id)
            .fetch_one(conn)
            .await?;
    Ok(ApproverGroup {
        id,
        name,
        members: members
            .into_iter()
            .map(|(id, full_name, email, role)| GroupMember {
                id,
                full_name,
                email,
                role,
            })
            .collect(),
        employees,
    })
}

#[utoipa::path(get, path = "/api/v1/admin/approver-groups", tag = "admin",
    responses(
        (status = 200, description = "Groups by name", body = [ApproverGroup]),
        (status = 403, description = "Not an admin", body = crate::openapi::ErrorBody),
    ))]
pub async fn list(
    State(state): State<AppState>,
    user: CurrentUser,
) -> ApiResult<Json<Vec<ApproverGroup>>> {
    user.require(Action::ManageUsers)?;
    let mut tx = begin_org_tx(&state.db, user.org_id).await?;
    let ids: Vec<Uuid> =
        sqlx::query_scalar("SELECT id FROM approver_groups ORDER BY lower(name), id")
            .fetch_all(&mut *tx)
            .await?;
    let mut out = Vec::with_capacity(ids.len());
    for id in ids {
        out.push(load(&mut tx, id).await?);
    }
    tx.commit().await?;
    Ok(Json(out))
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct GroupInput {
    /// 1–80 characters, unique in the organization.
    pub name: Option<String>,
    /// Approvers or admins (active or invited); replaces the members.
    pub member_ids: Option<Vec<Uuid>>,
}

/// Validate and write name and members.
async fn save(conn: &mut PgConnection, id: Uuid, input: &GroupInput) -> ApiResult<()> {
    if let Some(name) = &input.name {
        let name = name.trim();
        if name.is_empty() || name.chars().count() > 80 {
            return Err(field_error("name"));
        }
        let taken: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM approver_groups WHERE lower(name) = lower($1) AND id <> $2)",
        )
        .bind(name)
        .bind(id)
        .fetch_one(&mut *conn)
        .await?;
        if taken {
            return Err(field_error("name"));
        }
        sqlx::query("UPDATE approver_groups SET name = $2 WHERE id = $1")
            .bind(id)
            .bind(name)
            .execute(&mut *conn)
            .await?;
    }
    if let Some(members) = &input.member_ids {
        let eligible: i64 = sqlx::query_scalar(
            "SELECT count(DISTINCT id) FROM users
             WHERE id = ANY($1) AND role IN ('approver', 'admin') AND status <> 'deactivated'",
        )
        .bind(members)
        .fetch_one(&mut *conn)
        .await?;
        let distinct: std::collections::BTreeSet<&Uuid> = members.iter().collect();
        if eligible != distinct.len() as i64 {
            return Err(field_error("member_ids"));
        }
        let org: Uuid = sqlx::query_scalar("SELECT org_id FROM approver_groups WHERE id = $1")
            .bind(id)
            .fetch_one(&mut *conn)
            .await?;
        sqlx::query("DELETE FROM approver_group_members WHERE group_id = $1")
            .bind(id)
            .execute(&mut *conn)
            .await?;
        for m in distinct {
            sqlx::query(
                "INSERT INTO approver_group_members (group_id, user_id, org_id) VALUES ($1, $2, $3)",
            )
            .bind(id)
            .bind(m)
            .bind(org)
            .execute(&mut *conn)
            .await?;
        }
    }
    Ok(())
}

fn parse(body: Value) -> ApiResult<GroupInput> {
    serde_json::from_value(body).map_err(|e| {
        ApiError::new(ErrorCode::Validation).with_details(json!({ "body": e.to_string() }))
    })
}

async fn audit(
    conn: &mut PgConnection,
    user: &CurrentUser,
    action: &'static str,
    id: Uuid,
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
            entity_kind: "approver_group",
            entity_id: Some(id),
            before,
            after,
            ip: Some(peer.ip()),
        },
    )
    .await?;
    Ok(())
}

fn snapshot(g: &ApproverGroup) -> Value {
    json!({ "name": g.name, "member_ids": g.members.iter().map(|m| m.id).collect::<Vec<_>>() })
}

#[utoipa::path(post, path = "/api/v1/admin/approver-groups", tag = "admin", request_body = GroupInput,
    responses(
        (status = 201, description = "Created", body = ApproverGroup),
        (status = 403, description = "Not an admin", body = crate::openapi::ErrorBody),
        (status = 422, description = "VALIDATION (details.field: name — empty, too long or taken; member_ids — not approvers/admins)", body = crate::openapi::ErrorBody),
    ))]
pub async fn create(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    user: CurrentUser,
    ApiJson(body): ApiJson<Value>,
) -> ApiResult<(StatusCode, Json<ApproverGroup>)> {
    user.require(Action::ManageUsers)?;
    let input = parse(body)?;
    if input.name.is_none() {
        return Err(field_error("name"));
    }
    let mut tx = begin_org_tx(&state.db, user.org_id).await?;
    let id = Uuid::now_v7();
    sqlx::query("INSERT INTO approver_groups (id, org_id, name) VALUES ($1, $2, $3)")
        .bind(id)
        .bind(user.org_id)
        .bind(format!("new-{id}"))
        .execute(&mut *tx)
        .await?;
    save(&mut tx, id, &input).await?;
    let group = load(&mut tx, id).await?;
    audit(
        &mut tx,
        &user,
        "group.create",
        id,
        None,
        Some(snapshot(&group)),
        peer,
    )
    .await?;
    tx.commit().await?;
    Ok((StatusCode::CREATED, Json(group)))
}

#[utoipa::path(patch, path = "/api/v1/admin/approver-groups/{id}", tag = "admin", request_body = GroupInput,
    params(("id" = Uuid, Path, description = "Group id")),
    responses(
        (status = 200, description = "Updated", body = ApproverGroup),
        (status = 403, description = "Not an admin", body = crate::openapi::ErrorBody),
        (status = 404, description = "NOT_FOUND", body = crate::openapi::ErrorBody),
        (status = 422, description = "VALIDATION (details.field: name, member_ids)", body = crate::openapi::ErrorBody),
    ))]
pub async fn update(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    user: CurrentUser,
    Path(id): Path<Uuid>,
    ApiJson(body): ApiJson<Value>,
) -> ApiResult<Json<ApproverGroup>> {
    user.require(Action::ManageUsers)?;
    let input = parse(body)?;
    let mut tx = begin_org_tx(&state.db, user.org_id).await?;
    let before = load(&mut tx, id).await?;
    save(&mut tx, id, &input).await?;
    let group = load(&mut tx, id).await?;
    audit(
        &mut tx,
        &user,
        "group.update",
        id,
        Some(snapshot(&before)),
        Some(snapshot(&group)),
        peer,
    )
    .await?;
    tx.commit().await?;
    Ok(Json(group))
}

#[utoipa::path(delete, path = "/api/v1/admin/approver-groups/{id}", tag = "admin",
    params(("id" = Uuid, Path, description = "Group id")),
    responses(
        (status = 204, description = "Deleted; its employees have no group (their requests go to every approver)"),
        (status = 403, description = "Not an admin", body = crate::openapi::ErrorBody),
        (status = 404, description = "NOT_FOUND", body = crate::openapi::ErrorBody),
    ))]
pub async fn remove(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    user: CurrentUser,
    Path(id): Path<Uuid>,
) -> ApiResult<StatusCode> {
    user.require(Action::ManageUsers)?;
    let mut tx = begin_org_tx(&state.db, user.org_id).await?;
    let before = load(&mut tx, id).await?;
    sqlx::query("DELETE FROM approver_groups WHERE id = $1")
        .bind(id)
        .execute(&mut *tx)
        .await?;
    audit(
        &mut tx,
        &user,
        "group.delete",
        id,
        Some(snapshot(&before)),
        None,
        peer,
    )
    .await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}
