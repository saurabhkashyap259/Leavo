//! `GET /users/search` (T106): colleagues for the notify picker.
use crate::app::AppState;
use crate::auth::CurrentUser;
use crate::error::ApiResult;
use axum::extract::{Query, State};
use axum::Json;
use leavo_db::tx::begin_org_tx;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

#[derive(Serialize, ToSchema, sqlx::FromRow)]
pub struct UserHit {
    pub id: Uuid,
    pub full_name: String,
    pub email: String,
}

#[derive(Deserialize)]
pub struct SearchQuery {
    #[serde(default)]
    pub q: String,
}

/// `%`, `_` and `\` match literally in `ILIKE`.
fn like_escape(q: &str) -> String {
    q.chars()
        .flat_map(|c| match c {
            '%' | '_' | '\\' => vec!['\\', c],
            c => vec![c],
        })
        .collect()
}

#[utoipa::path(get, path = "/api/v1/users/search", tag = "me",
    params(("q" = String, Query, description = "At least 2 characters of a name or email")),
    responses(
        (status = 200, description = "Up to 10 active users of the same organization, by name", body = [UserHit]),
        (status = 401, description = "Not signed in", body = crate::openapi::ErrorBody),
    ))]
pub async fn search(
    State(state): State<AppState>,
    user: CurrentUser,
    Query(q): Query<SearchQuery>,
) -> ApiResult<Json<Vec<UserHit>>> {
    let q = q.q.trim();
    if q.chars().count() < 2 {
        return Ok(Json(vec![]));
    }
    let pattern = format!("%{}%", like_escape(q));
    let mut tx = begin_org_tx(&state.db, user.org_id).await?;
    let hits: Vec<UserHit> = sqlx::query_as(
        "SELECT id, full_name, email::text AS email FROM users
         WHERE status = 'active' AND anonymised_at IS NULL
           AND (full_name ILIKE $1 OR email::text ILIKE $1)
         ORDER BY lower(full_name), email LIMIT 10",
    )
    .bind(pattern)
    .fetch_all(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(Json(hits))
}
