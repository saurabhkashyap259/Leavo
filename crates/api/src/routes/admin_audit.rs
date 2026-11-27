//! Audit log (T187, FR-044): newest first, filtered by actor, entity, action and date range
//! (org time zone), paged with an opaque cursor.
use crate::app::AppState;
use crate::auth::CurrentUser;
use crate::error::{field_error, ApiResult};
use crate::routes::leaves::PersonRef;
use axum::extract::{Query, State};
use axum::Json;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use chrono::{DateTime, Days, NaiveDate, TimeZone, Utc};
use leavo_core::permissions::Action;
use leavo_db::leaves_repo;
use leavo_db::tx::begin_org_tx;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use utoipa::ToSchema;
use uuid::Uuid;

#[derive(Serialize, ToSchema)]
pub struct AuditItem {
    pub id: Uuid,
    /// Null for the system (scheduled jobs) and for setup before the admin exists.
    pub actor: Option<PersonRef>,
    /// Dotted verb, e.g. `leave.approve`, `balance.adjust`, `user.update`.
    pub action: String,
    pub entity_kind: String,
    pub entity_id: Option<Uuid>,
    #[schema(value_type = Object)]
    pub before: Option<Value>,
    #[schema(value_type = Object)]
    pub after: Option<Value>,
    pub ip: Option<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Serialize, ToSchema)]
pub struct AuditPage {
    pub items: Vec<AuditItem>,
    pub next_cursor: Option<String>,
}

#[derive(Deserialize)]
pub struct AuditQuery {
    pub actor: Option<String>,
    /// An entity id, or an entity kind such as `leave_request`.
    pub entity: Option<String>,
    pub action: Option<String>,
    pub from: Option<String>,
    pub to: Option<String>,
    pub cursor: Option<String>,
    pub limit: Option<String>,
}

#[derive(sqlx::FromRow)]
struct Row {
    id: Uuid,
    actor_id: Option<Uuid>,
    actor_name: Option<String>,
    action: String,
    entity_kind: String,
    entity_id: Option<Uuid>,
    before: Option<Value>,
    after: Option<Value>,
    ip: Option<String>,
    created_at: DateTime<Utc>,
}

fn encode(at: DateTime<Utc>, id: Uuid) -> String {
    URL_SAFE_NO_PAD.encode(format!("{}|{id}", at.to_rfc3339()))
}

fn decode(c: &str) -> Option<(DateTime<Utc>, Uuid)> {
    let raw = String::from_utf8(URL_SAFE_NO_PAD.decode(c).ok()?).ok()?;
    let (at, id) = raw.split_once('|')?;
    Some((
        DateTime::parse_from_rfc3339(at).ok()?.with_timezone(&Utc),
        id.parse().ok()?,
    ))
}

#[utoipa::path(get, path = "/api/v1/admin/audit-log", tag = "admin",
    params(
        ("actor" = Option<Uuid>, Query, description = "Who acted"),
        ("entity" = Option<String>, Query, description = "Entity id, or entity kind (e.g. `leave_request`)"),
        ("action" = Option<String>, Query, description = "e.g. `leave.approve`; a trailing `.` matches a prefix (`leave.`)"),
        ("from" = Option<String>, Query, description = "YYYY-MM-DD, org time zone, inclusive"),
        ("to" = Option<String>, Query, description = "YYYY-MM-DD, org time zone, inclusive"),
        ("cursor" = Option<String>, Query, description = "From the previous page"),
        ("limit" = Option<i64>, Query, description = "1–200, default 50"),
    ),
    responses(
        (status = 200, description = "Entries, newest first", body = AuditPage),
        (status = 403, description = "Not an admin", body = crate::openapi::ErrorBody),
        (status = 422, description = "VALIDATION (details.field)", body = crate::openapi::ErrorBody),
    ))]
pub async fn list(
    State(state): State<AppState>,
    user: CurrentUser,
    Query(q): Query<AuditQuery>,
) -> ApiResult<Json<AuditPage>> {
    user.require(Action::ViewReports)?;
    let actor: Option<Uuid> = match q.actor.as_deref() {
        None => None,
        Some(a) => Some(a.parse().map_err(|_| field_error("actor"))?),
    };
    let (entity_id, entity_kind): (Option<Uuid>, Option<String>) = match q.entity.as_deref() {
        None => (None, None),
        Some(e) => match e.parse::<Uuid>() {
            Ok(id) => (Some(id), None),
            Err(_) => (None, Some(e.to_string())),
        },
    };
    // `leave.` matches every `leave.*` action; anything else matches exactly.
    let (action, prefix) = match q.action.as_deref() {
        Some(a) if a.ends_with('.') => (None, Some(a.to_string())),
        Some(a) => (Some(a.to_string()), None),
        None => (None, None),
    };
    let date = |v: &Option<String>, field: &str| -> ApiResult<Option<NaiveDate>> {
        match v.as_deref() {
            None => Ok(None),
            Some(s) => Ok(Some(s.parse().map_err(|_| field_error(field))?)),
        }
    };
    let (from, to) = (date(&q.from, "from")?, date(&q.to, "to")?);
    let limit: i64 = match q.limit.as_deref() {
        None => 50,
        Some(l) => l
            .parse()
            .ok()
            .filter(|l| (1..=200).contains(l))
            .ok_or_else(|| field_error("limit"))?,
    };
    let after = match q.cursor.as_deref() {
        None => None,
        Some(c) => Some(decode(c).ok_or_else(|| field_error("cursor"))?),
    };
    let mut tx = begin_org_tx(&state.db, user.org_id).await?;
    let tz = leaves_repo::org_timezone(&mut tx, user.org_id).await?;
    let start = |d: NaiveDate| -> Option<DateTime<Utc>> {
        tz.from_local_datetime(&d.and_hms_opt(0, 0, 0)?)
            .earliest()
            .map(|t| t.with_timezone(&Utc))
    };
    let from_at = from.and_then(start);
    let to_at = to
        .and_then(|d| d.checked_add_days(Days::new(1)))
        .and_then(start);
    let rows: Vec<Row> = sqlx::query_as(
        "SELECT a.id, a.actor_id, u.full_name AS actor_name, a.action, a.entity_kind,
                a.entity_id, a.before, a.after, host(a.ip) AS ip, a.created_at
         FROM audit_log a LEFT JOIN users u ON u.id = a.actor_id
         WHERE ($1::uuid IS NULL OR a.actor_id = $1)
           AND ($2::uuid IS NULL OR a.entity_id = $2)
           AND ($3::text IS NULL OR a.entity_kind = $3)
           AND ($4::text IS NULL OR a.action = $4)
           AND ($10::text IS NULL OR starts_with(a.action, $10))
           AND ($5::timestamptz IS NULL OR a.created_at >= $5)
           AND ($6::timestamptz IS NULL OR a.created_at < $6)
           AND ($7::timestamptz IS NULL OR (a.created_at, a.id) < ($7, $8))
         ORDER BY a.created_at DESC, a.id DESC
         LIMIT $9",
    )
    .bind(actor)
    .bind(entity_id)
    .bind(entity_kind)
    .bind(action)
    .bind(from_at)
    .bind(to_at)
    .bind(after.map(|a| a.0))
    .bind(after.map(|a| a.1))
    .bind(limit + 1)
    .bind(prefix)
    .fetch_all(&mut *tx)
    .await?;
    tx.commit().await?;
    let more = rows.len() as i64 > limit;
    let items: Vec<AuditItem> = rows
        .into_iter()
        .take(limit as usize)
        .map(|r| AuditItem {
            id: r.id,
            actor: r
                .actor_id
                .zip(r.actor_name)
                .map(|(id, full_name)| PersonRef { id, full_name }),
            action: r.action,
            entity_kind: r.entity_kind,
            entity_id: r.entity_id,
            before: r.before,
            after: r.after,
            ip: r.ip,
            created_at: r.created_at,
        })
        .collect();
    let next_cursor = more
        .then(|| items.last().map(|i| encode(i.created_at, i.id)))
        .flatten();
    Ok(Json(AuditPage { items, next_cursor }))
}
