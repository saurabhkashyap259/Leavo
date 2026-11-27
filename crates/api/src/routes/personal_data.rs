//! Personal data routes (T188, FR-045): download your own data; admins export anyone's and
//! anonymise deactivated people.
use crate::app::AppState;
use crate::auth::CurrentUser;
use crate::error::{field_error, ApiResult};
use axum::extract::{ConnectInfo, Path, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use leavo_core::error::ErrorCode;
use leavo_core::permissions::Action;
use leavo_db::audit::{write_audit, AuditEntry};
use leavo_db::personal_data::{self, Anonymise};
use leavo_db::tx::begin_org_tx;
use serde_json::{json, Value};
use std::net::SocketAddr;
use uuid::Uuid;

#[utoipa::path(get, path = "/api/v1/me/export", tag = "me",
    responses(
        (status = 200, description = "Your profile, leave and comp-off history and balance entries as a JSON download", content_type = "application/json"),
        (status = 401, description = "Not signed in", body = crate::openapi::ErrorBody),
    ))]
pub async fn my_export(State(state): State<AppState>, user: CurrentUser) -> ApiResult<Response> {
    let mut tx = begin_org_tx(&state.db, user.org_id).await?;
    let data = personal_data::export(&mut tx, user.id, state.clock.now()).await?;
    tx.commit().await?;
    Ok((
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, "application/json".to_string()),
            (
                header::CONTENT_DISPOSITION,
                "attachment; filename=\"leavo-my-data.json\"".to_string(),
            ),
        ],
        Json(data),
    )
        .into_response())
}

#[utoipa::path(post, path = "/api/v1/admin/users/{id}/export", tag = "admin",
    params(("id" = Uuid, Path, description = "User id")),
    responses(
        (status = 200, description = "The person's data, as `/me/export` gives it to them", content_type = "application/json"),
        (status = 403, description = "Not an admin", body = crate::openapi::ErrorBody),
        (status = 404, description = "NOT_FOUND", body = crate::openapi::ErrorBody),
    ))]
pub async fn admin_export(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    user: CurrentUser,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Value>> {
    user.require(Action::ManageUsers)?;
    let mut tx = begin_org_tx(&state.db, user.org_id).await?;
    let exists: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM users WHERE id = $1)")
        .bind(id)
        .fetch_one(&mut *tx)
        .await?;
    if !exists {
        return Err(ErrorCode::NotFound.into());
    }
    let data = personal_data::export(&mut tx, id, state.clock.now()).await?;
    write_audit(
        &mut tx,
        &AuditEntry {
            org_id: user.org_id,
            actor_id: Some(user.id),
            action: "user.export",
            entity_kind: "user",
            entity_id: Some(id),
            before: None,
            after: None,
            ip: Some(peer.ip()),
        },
    )
    .await?;
    tx.commit().await?;
    Ok(Json(data))
}

#[utoipa::path(post, path = "/api/v1/admin/users/{id}/anonymise", tag = "admin",
    params(("id" = Uuid, Path, description = "User id")),
    responses(
        (status = 200, description = "Name and email replaced everywhere (audit log included), reasons, comments and attachments deleted; dates, days and balance entries kept"),
        (status = 403, description = "Not an admin", body = crate::openapi::ErrorBody),
        (status = 404, description = "NOT_FOUND", body = crate::openapi::ErrorBody),
        (status = 409, description = "ALREADY_ANONYMISED", body = crate::openapi::ErrorBody),
        (status = 422, description = "VALIDATION (details.field: status — deactivate the person first)", body = crate::openapi::ErrorBody),
    ))]
pub async fn anonymise(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    user: CurrentUser,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Value>> {
    user.require(Action::ManageUsers)?;
    let mut tx = begin_org_tx(&state.db, user.org_id).await?;
    let keys = match personal_data::anonymise(&mut tx, id, state.clock.now()).await? {
        Anonymise::NotFound => return Err(ErrorCode::NotFound.into()),
        Anonymise::AlreadyDone => return Err(ErrorCode::AlreadyAnonymised.into()),
        Anonymise::NotDeactivated => return Err(field_error("status")),
        Anonymise::Done { storage_keys } => storage_keys,
    };
    write_audit(
        &mut tx,
        &AuditEntry {
            org_id: user.org_id,
            actor_id: Some(user.id),
            action: "user.anonymise",
            entity_kind: "user",
            entity_id: Some(id),
            before: None,
            after: Some(json!({ "attachments_deleted": keys.len() })),
            ip: Some(peer.ip()),
        },
    )
    .await?;
    tx.commit().await?;
    // Stored objects go after the commit; a failure leaves an orphan object, never a dangling row.
    for key in &keys {
        if let Err(e) = state.storage.delete(key).await {
            tracing::warn!(error = %e, "could not delete an anonymised user's attachment");
        }
    }
    Ok(Json(json!({ "anonymised": true })))
}
