//! Create an organization with its first admin — shared by first-run setup (FR-007) and
//! multi-org registration (FR-008). Runs in one transaction.
use crate::error::{ApiError, ApiResult};
use crate::holiday_packs;
use chrono::{DateTime, Utc};
use leavo_core::error::ErrorCode;
use leavo_db::audit::{write_audit, AuditEntry};
use leavo_db::orgs_repo::{self, is_unique_violation, NewOrg};
use leavo_db::users_repo::{self, NewUserRow};
use leavo_db::{seed, tx};
use serde_json::json;
use sqlx::PgConnection;
use uuid::Uuid;

pub struct NewOrgWithAdmin<'a> {
    pub name: &'a str,
    pub timezone: &'a str,
    pub weekend_mask: i16,
    pub leave_year_start_month: i16,
    pub admin_email: &'a str,
    pub admin_name: &'a str,
    /// Already normalised; the first is the admin's own domain.
    pub domains: &'a [String],
    pub holiday_pack: Option<&'a str>,
    pub seed_leave_types: bool,
    pub audit_action: &'a str,
    pub ip: Option<std::net::IpAddr>,
}

/// Returns `(org_id, admin_user_id)`. Any domain already registered → `409 DOMAIN_TAKEN`.
pub async fn create(
    conn: &mut PgConnection,
    n: &NewOrgWithAdmin<'_>,
    now: DateTime<Utc>,
) -> ApiResult<(Uuid, Uuid)> {
    let org = Uuid::now_v7();
    tx::set_org(conn, org).await?;
    orgs_repo::insert_org(
        conn,
        &NewOrg {
            id: org,
            name: n.name,
            timezone: n.timezone,
            weekend_mask: n.weekend_mask,
            leave_year_start_month: n.leave_year_start_month,
        },
    )
    .await?;
    for d in n.domains {
        orgs_repo::add_domain(conn, org, d).await.map_err(|e| {
            if is_unique_violation(&e) {
                ApiError::new(ErrorCode::DomainTaken)
            } else {
                e.into()
            }
        })?;
    }
    if n.seed_leave_types {
        seed::seed_default_leave_types(conn, org).await?;
    }
    let mut holidays = 0;
    if let Some(pack) = n.holiday_pack {
        let rows: Vec<_> = holiday_packs::get(pack)
            .unwrap_or_default()
            .into_iter()
            .map(|h| (h.date, h.name, h.kind))
            .collect();
        holidays = orgs_repo::insert_holidays(conn, org, &rows, &format!("pack:{pack}")).await?;
    }
    let user = users_repo::insert_user(
        conn,
        &NewUserRow {
            org_id: org,
            email: n.admin_email,
            full_name: n.admin_name,
            role: "admin",
            status: "active",
            date_of_joining: None,
        },
        now,
    )
    .await
    .map_err(|e| {
        if is_unique_violation(&e) {
            ApiError::new(ErrorCode::DomainTaken)
        } else {
            e.into()
        }
    })?;
    let allotted = users_repo::allot_on_join(conn, org, user, None, now).await?;
    write_audit(
        conn,
        &AuditEntry {
            org_id: org,
            actor_id: Some(user),
            action: n.audit_action,
            entity_kind: "organization",
            entity_id: Some(org),
            before: None,
            after: Some(json!({
                "name": n.name, "timezone": n.timezone, "domains": n.domains,
                "holiday_pack": n.holiday_pack, "holidays": holidays,
                "seed_leave_types": n.seed_leave_types, "admin": n.admin_email,
                "allotments": allotted.len(),
            })),
            ip: n.ip,
        },
    )
    .await?;
    Ok((org, user))
}
