//! Attachments (T106, FR-029, FR-030): upload before submitting, 5-minute download links.
//!
//! Local storage downloads go through `/attachments/{id}/download`, protected by an HMAC
//! signature with an expiry instead of a session, so the link can be opened in a new tab.
use crate::app::AppState;
use crate::auth::CurrentUser;
use crate::error::{ApiError, ApiResult};
use crate::routes::leaves::can_view_request;
use axum::extract::{Multipart, Path, Query, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use chrono::{DateTime, Utc};
use leavo_core::error::ErrorCode;
use leavo_core::tokens::{sign_download, verify_download};
use leavo_db::tx::begin_org_tx;
use leavo_storage::{CappedBuffer, StorageError, MAX_ATTACHMENT_BYTES, SIGNED_URL_TTL};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

fn storage_error(e: StorageError) -> ApiError {
    match e {
        StorageError::TooLarge => ErrorCode::FileTooLarge.into(),
        StorageError::TypeNotAllowed => ErrorCode::FileTypeNotAllowed.into(),
        StorageError::NotFound => ErrorCode::NotFound.into(),
        StorageError::Backend(msg) => ApiError::internal(msg),
    }
}

/// The last path segment, without control characters, at most 255 characters.
fn clean_file_name(raw: Option<&str>) -> String {
    let base = raw
        .unwrap_or("")
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or("")
        .chars()
        .filter(|c| !c.is_control() && *c != '"')
        .take(255)
        .collect::<String>();
    let base = base.trim();
    if base.is_empty() {
        "file".into()
    } else {
        base.to_string()
    }
}

#[derive(Serialize, ToSchema)]
pub struct Uploaded {
    pub id: Uuid,
    pub name: String,
    pub content_type: String,
    pub size: usize,
}

#[derive(ToSchema)]
#[allow(dead_code)]
pub struct UploadForm {
    /// PDF, PNG, JPEG, WebP or HEIC, up to 5 MB. The type is detected from the content.
    #[schema(format = Binary, value_type = String)]
    file: Vec<u8>,
}

#[utoipa::path(post, path = "/api/v1/attachments", tag = "leaves",
    request_body(content = UploadForm, content_type = "multipart/form-data"),
    responses(
        (status = 201, description = "Stored; pass `id` as `attachment_id` when creating the request", body = Uploaded),
        (status = 401, description = "Not signed in", body = crate::openapi::ErrorBody),
        (status = 413, description = "FILE_TOO_LARGE", body = crate::openapi::ErrorBody),
        (status = 415, description = "FILE_TYPE_NOT_ALLOWED", body = crate::openapi::ErrorBody),
        (status = 422, description = "VALIDATION: no `file` part", body = crate::openapi::ErrorBody),
    ))]
pub async fn upload(
    State(state): State<AppState>,
    user: CurrentUser,
    mut form: Multipart,
) -> ApiResult<Response> {
    let mut file = None;
    while let Some(mut field) = form
        .next_field()
        .await
        .map_err(|_| ApiError::new(ErrorCode::FileTooLarge))?
    {
        if field.name() != Some("file") {
            continue;
        }
        let name = clean_file_name(field.file_name());
        let mut buf = CappedBuffer::new(MAX_ATTACHMENT_BYTES);
        // A body over the server limit surfaces as a chunk error: also "too large".
        while let Some(chunk) = field
            .chunk()
            .await
            .map_err(|_| ApiError::new(ErrorCode::FileTooLarge))?
        {
            buf.push(&chunk).map_err(storage_error)?;
        }
        file = Some((name, buf.into_bytes()));
        break;
    }
    let (name, bytes) = file.ok_or_else(|| crate::error::field_error("file"))?;
    let id = Uuid::now_v7();
    let key = format!("{}/{}/{id}", user.org_id, user.id);
    let size = bytes.len();
    let content_type = state
        .storage
        .put_checked(&key, bytes)
        .await
        .map_err(storage_error)?;
    let mut tx = begin_org_tx(&state.db, user.org_id).await?;
    sqlx::query(
        "INSERT INTO attachments (id, org_id, owner_user_id, storage_key, file_name, content_type, size_bytes, created_at)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
    )
    .bind(id)
    .bind(user.org_id)
    .bind(user.id)
    .bind(&key)
    .bind(&name)
    .bind(content_type)
    .bind(size as i32)
    .bind(state.clock.now())
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok((
        StatusCode::CREATED,
        Json(Uploaded {
            id,
            name,
            content_type: content_type.into(),
            size,
        }),
    )
        .into_response())
}

#[derive(Serialize, ToSchema)]
pub struct SignedUrl {
    pub url: String,
    pub expires_at: DateTime<Utc>,
}

#[utoipa::path(get, path = "/api/v1/attachments/{id}/url", tag = "leaves",
    params(("id" = Uuid, Path, description = "Attachment id")),
    responses(
        (status = 200, description = "A download link valid for 5 minutes", body = SignedUrl),
        (status = 401, description = "Not signed in", body = crate::openapi::ErrorBody),
        (status = 404, description = "Not found, or not visible: only the owner, the request's eligible approvers and admins (FR-029)", body = crate::openapi::ErrorBody),
    ))]
pub async fn signed_url(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<SignedUrl>> {
    let mut tx = begin_org_tx(&state.db, user.org_id).await?;
    // (owner, storage key, request, requester, routed group)
    #[allow(clippy::type_complexity)]
    let row: Option<(Uuid, String, Option<Uuid>, Option<Uuid>, Option<Uuid>)> = sqlx::query_as(
        "SELECT a.owner_user_id, a.storage_key, a.request_id, r.user_id, r.routed_group_id
         FROM attachments a LEFT JOIN leave_requests r ON r.id = a.request_id
         WHERE a.id = $1",
    )
    .bind(id)
    .fetch_optional(&mut *tx)
    .await?;
    let Some((owner, key, request, requester, routed)) = row else {
        return Err(ErrorCode::NotFound.into());
    };
    let visible = owner == user.id
        || match (request, requester) {
            (Some(_), Some(requester)) => {
                can_view_request(&mut tx, &user, requester, routed).await?
            }
            _ => false,
        };
    tx.commit().await?;
    if !visible {
        return Err(ErrorCode::NotFound.into());
    }
    let now = state.clock.now();
    let expires_at = now + chrono::Duration::seconds(SIGNED_URL_TTL.as_secs() as i64);
    let url = match state
        .storage
        .presigned_get(&key)
        .await
        .map_err(storage_error)?
    {
        Some(url) => url,
        None => {
            let exp = expires_at.timestamp();
            let sig = sign_download(state.config.secret_key.as_bytes(), user.org_id, id, exp);
            format!(
                "{}/api/v1/attachments/{id}/download?o={}&exp={exp}&sig={sig}",
                state.config.base_url, user.org_id
            )
        }
    };
    Ok(Json(SignedUrl { url, expires_at }))
}

#[derive(Deserialize)]
pub struct DownloadQuery {
    pub o: Uuid,
    pub exp: i64,
    pub sig: String,
}

#[utoipa::path(get, path = "/api/v1/attachments/{id}/download", tag = "leaves",
    params(
        ("id" = Uuid, Path, description = "Attachment id"),
        ("o" = Uuid, Query, description = "Organization id"),
        ("exp" = i64, Query, description = "Expiry (unix seconds)"),
        ("sig" = String, Query, description = "Signature"),
    ),
    responses(
        (status = 200, description = "The file (local storage only; S3 links go straight to the bucket)", content_type = "application/octet-stream"),
        (status = 403, description = "FORBIDDEN: expired or invalid link", body = crate::openapi::ErrorBody),
        (status = 404, description = "NOT_FOUND", body = crate::openapi::ErrorBody),
    ))]
pub async fn download(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Query(q): Query<DownloadQuery>,
) -> ApiResult<Response> {
    let now = state.clock.now().timestamp();
    if !verify_download(
        state.config.secret_key.as_bytes(),
        q.o,
        id,
        q.exp,
        &q.sig,
        now,
    ) {
        return Err(ErrorCode::Forbidden.into());
    }
    let mut tx = begin_org_tx(&state.db, q.o).await?;
    let row: Option<(String, String, String)> = sqlx::query_as(
        "SELECT storage_key, content_type, file_name FROM attachments WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(&mut *tx)
    .await?;
    tx.commit().await?;
    let (key, content_type, name) = row.ok_or(ApiError::new(ErrorCode::NotFound))?;
    let bytes = state.storage.get(&key).await.map_err(storage_error)?;
    let ascii: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_graphic() || c == ' ' {
                c
            } else {
                '_'
            }
        })
        .collect();
    Ok((
        [
            (header::CONTENT_TYPE, content_type),
            (
                header::CONTENT_DISPOSITION,
                format!("attachment; filename=\"{ascii}\""),
            ),
            (header::CACHE_CONTROL, "private, no-store".into()),
            (header::X_CONTENT_TYPE_OPTIONS, "nosniff".into()),
        ],
        bytes,
    )
        .into_response())
}
