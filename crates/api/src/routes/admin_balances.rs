//! Balances seen by admins and manual adjustments (T184, FR-040, FR-041).
use crate::app::AppState;
use crate::auth::CurrentUser;
use crate::error::{field_error, ApiError, ApiResult};
use crate::extract::ApiJson;
use crate::routes::leaves::{num, PersonRef};
use crate::routes::me::{balance_cards, Balance};
use axum::extract::{ConnectInfo, Path, Query, State};
use axum::http::StatusCode;
use axum::Json;
use chrono::{DateTime, Utc};
use leavo_core::error::ErrorCode;
use leavo_core::permissions::Action;
use leavo_db::audit::{write_audit, AuditEntry};
use leavo_db::ledger::{insert_entry, LedgerEntry, LedgerReason};
use leavo_db::tx::{begin_org_tx, lock_balance};
use leavo_db::{leave_types_repo, leaves_repo};
use rust_decimal::prelude::FromPrimitive;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::PgConnection;
use std::net::SocketAddr;
use utoipa::ToSchema;
use uuid::Uuid;

#[derive(Serialize, ToSchema)]
pub struct LedgerType {
    pub id: Uuid,
    pub code: String,
    pub name: String,
}

/// One balance entry (FR-040).
#[derive(Serialize, ToSchema)]
pub struct LedgerItem {
    pub id: Uuid,
    pub leave_type: LedgerType,
    pub leave_year: i32,
    pub delta: f64,
    /// `allotment`, `prorate`, `quota_change`, `carry_forward`, `negative_carry`, `lapse`,
    /// `leave_taken`, `leave_reversed`, `comp_off_credit`, `comp_off_expired`,
    /// `comp_off_revoked`, `manual_adjustment` or `import`.
    pub reason: String,
    pub note: Option<String>,
    pub ref_kind: Option<String>,
    pub ref_id: Option<Uuid>,
    pub created_by: Option<PersonRef>,
    pub created_at: DateTime<Utc>,
}

#[derive(sqlx::FromRow)]
struct LedgerRow {
    id: Uuid,
    type_id: Uuid,
    code: String,
    name: String,
    leave_year: i32,
    delta: Decimal,
    reason: String,
    note: Option<String>,
    ref_kind: Option<String>,
    ref_id: Option<Uuid>,
    created_by: Option<Uuid>,
    created_by_name: Option<String>,
    created_at: DateTime<Utc>,
}

impl From<LedgerRow> for LedgerItem {
    fn from(r: LedgerRow) -> Self {
        Self {
            id: r.id,
            leave_type: LedgerType {
                id: r.type_id,
                code: r.code,
                name: r.name,
            },
            leave_year: r.leave_year,
            delta: num(r.delta),
            reason: r.reason,
            note: r.note,
            ref_kind: r.ref_kind,
            ref_id: r.ref_id,
            created_by: r
                .created_by
                .zip(r.created_by_name)
                .map(|(id, full_name)| PersonRef { id, full_name }),
            created_at: r.created_at,
        }
    }
}

const LEDGER: &str = "SELECT l.id, t.id AS type_id, t.code, t.name, l.leave_year, l.delta,
        l.reason::text AS reason, l.note, l.ref_kind, l.ref_id, l.created_by,
        u.full_name AS created_by_name, l.created_at
    FROM balance_ledger l
    JOIN leave_types t ON t.id = l.leave_type_id
    LEFT JOIN users u ON u.id = l.created_by";

#[derive(Serialize, ToSchema)]
pub struct UserBalances {
    pub balances: Vec<Balance>,
    /// Entries of the leave year (comp-off entries of every year), oldest first.
    pub ledger: Vec<LedgerItem>,
}

#[derive(Deserialize)]
pub struct YearQuery {
    pub year: Option<String>,
}

async fn require_user(conn: &mut PgConnection, id: Uuid) -> ApiResult<()> {
    let exists: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM users WHERE id = $1)")
        .bind(id)
        .fetch_one(conn)
        .await?;
    if exists {
        Ok(())
    } else {
        Err(ErrorCode::NotFound.into())
    }
}

#[utoipa::path(get, path = "/api/v1/admin/users/{id}/balances", tag = "admin",
    params(
        ("id" = Uuid, Path, description = "User id"),
        ("year" = Option<i32>, Query, description = "Leave year; defaults to the current one"),
    ),
    responses(
        (status = 200, description = "The person's balance cards and the entries behind them", body = UserBalances),
        (status = 403, description = "Not an admin", body = crate::openapi::ErrorBody),
        (status = 404, description = "NOT_FOUND", body = crate::openapi::ErrorBody),
    ))]
pub async fn balances(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<Uuid>,
    Query(q): Query<YearQuery>,
) -> ApiResult<Json<UserBalances>> {
    user.require(Action::AdjustBalances)?;
    let mut tx = begin_org_tx(&state.db, user.org_id).await?;
    require_user(&mut tx, id).await?;
    let cal = leaves_repo::org_calendar(&mut tx, user.org_id, state.clock.now()).await?;
    let year = match q.year.as_deref() {
        None => cal.current_leave_year(),
        Some(y) => y
            .parse()
            .ok()
            .filter(|y| (2000..=2200).contains(y))
            .ok_or_else(|| field_error("year"))?,
    };
    let balances = balance_cards(&mut tx, id, year, &cal).await?;
    let rows: Vec<LedgerRow> = sqlx::query_as(&format!(
        "{LEDGER} WHERE l.user_id = $1 AND (l.leave_year = $2 OR t.is_comp_off)
         ORDER BY l.created_at, l.id"
    ))
    .bind(id)
    .bind(year)
    .fetch_all(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(Json(UserBalances {
        balances,
        ledger: rows.into_iter().map(Into::into).collect(),
    }))
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct AdjustmentInput {
    pub leave_type_id: Uuid,
    pub leave_year: i32,
    /// Non-zero, a multiple of 0.5; negative deducts.
    pub delta: f64,
    /// Required: why (FR-041).
    pub note: Option<String>,
}

#[utoipa::path(post, path = "/api/v1/admin/users/{id}/adjustments", tag = "admin",
    params(("id" = Uuid, Path, description = "User id")), request_body = AdjustmentInput,
    responses(
        (status = 201, description = "A `manual_adjustment` entry recorded with its author", body = LedgerItem),
        (status = 403, description = "Not an admin", body = crate::openapi::ErrorBody),
        (status = 404, description = "NOT_FOUND", body = crate::openapi::ErrorBody),
        (status = 422, description = "NOTE_REQUIRED; VALIDATION (details.field: delta, leave_year, leave_type_id, note)", body = crate::openapi::ErrorBody),
    ))]
pub async fn adjust(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    user: CurrentUser,
    Path(id): Path<Uuid>,
    ApiJson(body): ApiJson<Value>,
) -> ApiResult<(StatusCode, Json<LedgerItem>)> {
    user.require(Action::AdjustBalances)?;
    let input: AdjustmentInput = serde_json::from_value(body).map_err(|e| {
        ApiError::new(ErrorCode::Validation).with_details(json!({ "body": e.to_string() }))
    })?;
    let note = input
        .note
        .as_deref()
        .map(str::trim)
        .filter(|n| !n.is_empty())
        .ok_or_else(|| ApiError::new(ErrorCode::NoteRequired))?
        .to_string();
    if note.chars().count() > 500 {
        return Err(field_error("note"));
    }
    let delta = Decimal::from_f64(input.delta)
        .map(|d| d.round_dp(1))
        .filter(|d| {
            !d.is_zero() && (*d * Decimal::TWO).fract().is_zero() && d.abs() <= Decimal::from(400)
        })
        .ok_or_else(|| field_error("delta"))?;
    if !(2000..=2200).contains(&input.leave_year) {
        return Err(field_error("leave_year"));
    }
    let mut tx = begin_org_tx(&state.db, user.org_id).await?;
    require_user(&mut tx, id).await?;
    let t = leave_types_repo::get(&mut tx, input.leave_type_id)
        .await?
        .ok_or_else(|| field_error("leave_type_id"))?;
    lock_balance(&mut tx, id, t.id).await?;
    let year = (!t.is_comp_off).then_some(input.leave_year);
    let before = leaves_repo::balance_row(&mut tx, id, t.id, year).await?;
    let entry_id = insert_entry(
        &mut tx,
        &LedgerEntry {
            org_id: user.org_id,
            user_id: id,
            leave_type_id: t.id,
            leave_year: input.leave_year,
            delta,
            reason: LedgerReason::ManualAdjustment,
            ref_kind: None,
            ref_id: None,
            note: Some(note.clone()),
            created_by: Some(user.id),
        },
    )
    .await?;
    let after = leaves_repo::balance_row(&mut tx, id, t.id, year).await?;
    write_audit(
        &mut tx,
        &AuditEntry {
            org_id: user.org_id,
            actor_id: Some(user.id),
            action: "balance.adjust",
            entity_kind: "user",
            entity_id: Some(id),
            before: Some(
                json!({ "leave_type": t.code, "leave_year": input.leave_year,
                                 "balance": num(before.balance) }),
            ),
            after: Some(
                json!({ "leave_type": t.code, "leave_year": input.leave_year,
                                "balance": num(after.balance), "delta": num(delta), "note": note }),
            ),
            ip: Some(peer.ip()),
        },
    )
    .await?;
    let row: LedgerRow = sqlx::query_as(&format!("{LEDGER} WHERE l.id = $1"))
        .bind(entry_id)
        .fetch_one(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok((StatusCode::CREATED, Json(row.into())))
}
