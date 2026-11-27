//! Admin dashboard (T185, FR-036, FR-044): who is away today (with type), what waits for a
//! decision, upcoming holidays, the next-year holiday reminder and setup progress.
use crate::app::AppState;
use crate::auth::CurrentUser;
use crate::error::ApiResult;
use crate::routes::holidays::HolidayItem;
use crate::routes::leaves::{num, PersonRef, TypeRef};
use axum::extract::State;
use axum::Json;
use chrono::{Duration, Months, NaiveDate};
use leavo_core::calendar::leave_year_bounds;
use leavo_core::permissions::Action;
use leavo_db::tx::begin_org_tx;
use leavo_db::{away_repo, holidays_repo, leaves_repo};
use serde::Serialize;
use utoipa::ToSchema;

#[derive(Serialize, ToSchema)]
pub struct AwayToday {
    pub user: PersonRef,
    pub from: NaiveDate,
    pub to: NaiveDate,
    pub days: f64,
    pub leave_type: TypeRef,
}

/// Facts behind the setup checklist; the page combines them with what the admin has opened.
#[derive(Serialize, ToSchema)]
pub struct SetupChecklist {
    /// This leave year has holidays.
    pub holidays: bool,
    /// At least one approver group exists.
    pub approvers: bool,
    /// Someone besides admins has been invited or signed up.
    pub invite: bool,
}

#[derive(Serialize, ToSchema)]
pub struct AdminDashboard {
    pub away_today: Vec<AwayToday>,
    /// Leave, comp-off and cancellation requests waiting for a decision.
    pub pending: i64,
    /// Of those, waiting longer than the reminder threshold (48 h by default).
    pub pending_over_48h: i64,
    /// The next 3, from today.
    pub upcoming_holidays: Vec<HolidayItem>,
    /// From two months before the next leave year starts (1 November for a January year), true
    /// while next year has no holidays (FR-036).
    pub next_year_holidays_missing: bool,
    pub setup_checklist: SetupChecklist,
}

#[utoipa::path(get, path = "/api/v1/admin/dashboard", tag = "admin",
    responses(
        (status = 200, description = "Org dashboard", body = AdminDashboard),
        (status = 403, description = "Not an admin", body = crate::openapi::ErrorBody),
    ))]
pub async fn dashboard(
    State(state): State<AppState>,
    user: CurrentUser,
) -> ApiResult<Json<AdminDashboard>> {
    user.require(Action::ViewReports)?;
    let now = state.clock.now();
    let mut tx = begin_org_tx(&state.db, user.org_id).await?;
    let cal = leaves_repo::org_calendar(&mut tx, user.org_id, now).await?;
    let away = away_repo::requests_between(&mut tx, None, cal.today, cal.today).await?;
    let reminder_hours: i32 =
        sqlx::query_scalar("SELECT reminder_after_hours FROM organizations WHERE id = $1")
            .bind(user.org_id)
            .fetch_one(&mut *tx)
            .await?;
    let cutoff = now - Duration::hours(reminder_hours.into());
    // (waiting since) of everything pending a decision.
    let waiting: Vec<chrono::DateTime<chrono::Utc>> = sqlx::query_scalar(
        "SELECT created_at FROM leave_requests WHERE status = 'pending'
         UNION ALL SELECT cancel_requested_at FROM leave_requests WHERE status = 'cancel_requested'
         UNION ALL SELECT created_at FROM comp_off_requests WHERE status = 'pending'",
    )
    .fetch_all(&mut *tx)
    .await?;
    let year = cal.current_leave_year();
    let (_, this_end) = leave_year_bounds(year, cal.start_month);
    let (next_start, next_end) = leave_year_bounds(year + 1, cal.start_month);
    let upcoming = holidays_repo::between(&mut tx, cal.today, next_end).await?;
    let next_count = holidays_repo::between(&mut tx, next_start, next_end)
        .await?
        .len();
    let remind_from = next_start - Months::new(2);
    let (this_start, _) = leave_year_bounds(year, cal.start_month);
    let this_year_holidays = !holidays_repo::between(&mut tx, this_start, this_end)
        .await?
        .is_empty();
    let groups: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM approver_groups)")
        .fetch_one(&mut *tx)
        .await?;
    let invited: bool =
        sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM users WHERE role <> 'admin')")
            .fetch_one(&mut *tx)
            .await?;
    tx.commit().await?;
    Ok(Json(AdminDashboard {
        away_today: away
            .into_iter()
            .map(|a| AwayToday {
                user: PersonRef {
                    id: a.user_id,
                    full_name: a.full_name,
                },
                from: a.from_date,
                to: a.to_date,
                days: num(a.days),
                leave_type: TypeRef {
                    id: a.leave_type_id,
                    code: a.code,
                    name: a.name,
                    color: a.color,
                },
            })
            .collect(),
        pending: waiting.len() as i64,
        pending_over_48h: waiting.iter().filter(|t| **t <= cutoff).count() as i64,
        upcoming_holidays: upcoming.into_iter().take(3).map(Into::into).collect(),
        next_year_holidays_missing: cal.today >= remind_from && next_count == 0,
        setup_checklist: SetupChecklist {
            holidays: this_year_holidays,
            approvers: groups,
            invite: invited,
        },
    }))
}
