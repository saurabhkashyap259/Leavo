//! Imports (T199, FR-043): `POST /admin/import/{csv|keka}?dry_run=` with multipart parts
//! `balances` (required), `history` (optional) and, for Keka, `leave_type_map` (JSON object of
//! Keka name → Leavo code) and `leave_year`. The dry run reports the plan; the apply writes it
//! all in one transaction, and only when the plan has no errors.
use crate::app::AppState;
use crate::auth::CurrentUser;
use crate::error::{field_error, ApiError, ApiResult};
use axum::extract::{ConnectInfo, Multipart, Path, Query, State};
use axum::Json;
use chrono::NaiveDate;
use leavo_core::error::ErrorCode;
use leavo_core::permissions::Action;
use leavo_db::audit::{write_audit, AuditEntry};
use leavo_db::tx::begin_org_tx;
use leavo_db::{import_repo, leaves_repo};
use leavo_importers::csv::{parse_balances, parse_history, plan, Issue};
use leavo_importers::keka::{keka_leave_types, map_keka, KekaMap};
use rust_decimal::prelude::ToPrimitive;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::BTreeMap;
use std::net::SocketAddr;
use utoipa::ToSchema;

#[derive(Deserialize)]
pub struct ImportQuery {
    pub dry_run: Option<bool>,
}

#[derive(Serialize, ToSchema)]
pub struct ImportUser {
    pub email: String,
    pub full_name: String,
    pub approver_group: Option<String>,
}

#[derive(Serialize, ToSchema)]
pub struct ImportBalance {
    pub row: usize,
    pub email: String,
    pub leave_type_code: String,
    pub leave_year: i32,
    pub available_days: f64,
}

#[derive(Serialize, ToSchema)]
pub struct ImportHistory {
    pub row: usize,
    pub email: String,
    pub leave_type_code: String,
    pub from: NaiveDate,
    pub to: NaiveDate,
    pub days: f64,
}

#[derive(Serialize, ToSchema)]
pub struct ImportRowIssue {
    /// `balances` or `history`.
    pub file: String,
    /// 1-based, counting the header line.
    pub row: usize,
    pub code: String,
    pub message: String,
}

impl From<Issue> for ImportRowIssue {
    fn from(i: Issue) -> Self {
        Self {
            file: i.file.into(),
            row: i.row,
            code: i.code.into(),
            message: i.message,
        }
    }
}

#[derive(Serialize, ToSchema)]
pub struct KekaType {
    /// The leave type name in the Keka files.
    pub name: String,
    /// The Leavo code it maps to (suggested from matching names, or from `leave_type_map`).
    pub code: Option<String>,
}

#[derive(Serialize, ToSchema)]
pub struct ImportReport {
    pub dry_run: bool,
    pub users_to_create: Vec<ImportUser>,
    pub groups_to_create: Vec<String>,
    pub balances_to_set: Vec<ImportBalance>,
    pub history_rows: Vec<ImportHistory>,
    /// `UNKNOWN_DOMAIN`, `UNKNOWN_LEAVE_TYPE`, `UNKNOWN_USER`, `INVALID_DATE`, `INVALID_DAYS`,
    /// `DUPLICATE_ROW`, `MISSING_COLUMN`, `MISSING_VALUE`, `OVERLAP`. Any error blocks the apply.
    pub errors: Vec<ImportRowIssue>,
    /// `USER_EXISTS`, `GROUP_WILL_BE_CREATED`, `BALANCE_REPLACES_EARLIER_IMPORT`,
    /// `HISTORY_OUTSIDE_LEAVE_YEAR`.
    pub warnings: Vec<ImportRowIssue>,
    /// Keka only: each leave type name found and the code it maps to.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub keka_leave_types: Option<Vec<KekaType>>,
    /// Keka only: history rows not approved in Keka, left out.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skipped_history: Option<usize>,
}

fn num(d: rust_decimal::Decimal) -> f64 {
    d.to_f64().unwrap_or_default()
}

#[utoipa::path(post, path = "/api/v1/admin/import/{kind}", tag = "admin",
    params(
        ("kind" = String, Path, description = "`csv` (contracts/csv-formats.md §2–3) or `keka` (§4)"),
        ("dry_run" = Option<bool>, Query, description = "true: report only, write nothing"),
    ),
    request_body(content_type = "multipart/form-data", description = "`balances` file (required), `history` file (optional); Keka: `leave_type_map` (JSON object name → code), `leave_year` (default: current)"),
    responses(
        (status = 200, description = "The plan (dry run) or what was applied", body = ImportReport),
        (status = 403, description = "Not an admin", body = crate::openapi::ErrorBody),
        (status = 404, description = "Unknown kind", body = crate::openapi::ErrorBody),
        (status = 422, description = "IMPORT_HAS_ERRORS (apply with any error row; nothing written); VALIDATION (details.field: balances, leave_type_map, leave_year)", body = crate::openapi::ErrorBody),
    ))]
pub async fn import(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    user: CurrentUser,
    Path(kind): Path<String>,
    Query(q): Query<ImportQuery>,
    mut form: Multipart,
) -> ApiResult<Json<ImportReport>> {
    user.require(Action::ManageUsers)?;
    let keka = match kind.as_str() {
        "csv" => false,
        "keka" => true,
        _ => return Err(ErrorCode::NotFound.into()),
    };
    let dry_run = q.dry_run.unwrap_or(false);
    let mut parts: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    while let Some(field) = form
        .next_field()
        .await
        .map_err(|_| field_error("balances"))?
    {
        let name = field.name().unwrap_or_default().to_string();
        if ["balances", "history", "leave_type_map", "leave_year"].contains(&name.as_str()) {
            let bytes = field.bytes().await.map_err(|_| field_error(&name))?;
            parts.insert(name, bytes.to_vec());
        }
    }
    let balances = parts
        .get("balances")
        .ok_or_else(|| field_error("balances"))?;
    let history = parts.get("history");

    let now = state.clock.now();
    let mut tx = begin_org_tx(&state.db, user.org_id).await?;
    let cal = leaves_repo::org_calendar(&mut tx, user.org_id, now).await?;
    let ctx = import_repo::context(&mut tx, &cal).await?;

    let mut keka_types = None;
    let mut skipped = None;
    let (rows, hist, issues) = if keka {
        let leave_year = match parts.get("leave_year") {
            None => cal.current_leave_year(),
            Some(y) => String::from_utf8_lossy(y)
                .trim()
                .parse::<i32>()
                .ok()
                .filter(|y| (2000..=2200).contains(y))
                .ok_or_else(|| field_error("leave_year"))?,
        };
        let given: BTreeMap<String, String> = match parts.get("leave_type_map") {
            None => BTreeMap::new(),
            Some(raw) if raw.iter().all(u8::is_ascii_whitespace) => BTreeMap::new(),
            Some(raw) => serde_json::from_slice(raw).map_err(|_| field_error("leave_type_map"))?,
        };
        // Suggest by matching code or name; the admin's map wins.
        let names: Vec<(String, String)> =
            sqlx::query_as("SELECT upper(code), lower(name) FROM leave_types")
                .fetch_all(&mut *tx)
                .await?;
        let found = keka_leave_types(balances, history.map(Vec::as_slice));
        let mut map = KekaMap::new();
        let mut listed = Vec::new();
        for name in found {
            let lower = name.to_lowercase();
            let code = given
                .iter()
                .find(|(k, _)| k.to_lowercase() == lower)
                .map(|(_, v)| v.to_uppercase())
                .filter(|c| !c.is_empty())
                .or_else(|| {
                    names
                        .iter()
                        .find(|(code, n)| *n == lower || code.to_lowercase() == lower)
                        .map(|(code, _)| code.clone())
                });
            if let Some(c) = &code {
                map.insert(lower, c.clone());
            }
            listed.push(KekaType { name, code });
        }
        keka_types = Some(listed);
        let out = map_keka(balances, history.map(Vec::as_slice), &map, leave_year);
        skipped = history.map(|_| out.skipped_history);
        (out.balances, out.history, out.issues)
    } else {
        let (rows, mut issues) = parse_balances(balances);
        let (hist, more) = match history {
            Some(h) => parse_history(h),
            None => (vec![], vec![]),
        };
        issues.extend(more);
        (rows, hist, issues)
    };
    let p = plan(rows, hist, issues, &ctx);

    let report = ImportReport {
        dry_run,
        users_to_create: p
            .users_to_create
            .iter()
            .map(|u| ImportUser {
                email: u.email.clone(),
                full_name: u.full_name.clone(),
                approver_group: u.approver_group.clone(),
            })
            .collect(),
        groups_to_create: p.groups_to_create.clone(),
        balances_to_set: p
            .balances
            .iter()
            .map(|b| ImportBalance {
                row: b.row,
                email: b.email.clone(),
                leave_type_code: b.leave_type_code.clone(),
                leave_year: b.leave_year,
                available_days: num(b.available_days),
            })
            .collect(),
        history_rows: p
            .history
            .iter()
            .map(|h| ImportHistory {
                row: h.row,
                email: h.email.clone(),
                leave_type_code: h.leave_type_code.clone(),
                from: h.from_date,
                to: h.to_date,
                days: num(h.days),
            })
            .collect(),
        errors: p.errors.iter().cloned().map(Into::into).collect(),
        warnings: p.warnings.iter().cloned().map(Into::into).collect(),
        keka_leave_types: keka_types,
        skipped_history: skipped,
    };
    if dry_run {
        tx.commit().await?;
        return Ok(Json(report));
    }
    if !p.errors.is_empty() {
        return Err(ApiError::new(ErrorCode::ImportHasErrors)
            .with_details(json!({ "errors": p.errors.len() })));
    }
    let applied = import_repo::apply(&mut tx, user.org_id, user.id, &p, &ctx, &cal, now).await?;
    write_audit(
        &mut tx,
        &AuditEntry {
            org_id: user.org_id,
            actor_id: Some(user.id),
            action: "import.apply",
            entity_kind: "import",
            entity_id: None,
            before: None,
            after: Some(json!({
                "source": kind, "users_created": applied.users_created,
                "groups_created": applied.groups_created, "balance_entries": applied.entries,
                "history_requests": applied.requests, "warnings": p.warnings.len(),
            })),
            ip: Some(peer.ip()),
        },
    )
    .await?;
    tx.commit().await?;
    Ok(Json(report))
}
