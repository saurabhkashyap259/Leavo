//! Who's away and the team calendar (T131, FR-027, FR-058, research R10).
//!
//! Everyone sees name, dates and days. Leave type and reason are included only for viewers
//! allowed to see them — the person's eligible approvers (by current group) and admins; for
//! everyone else the keys are absent.
use crate::app::AppState;
use crate::auth::CurrentUser;
use crate::error::{field_error, ApiResult};
use crate::routes::approvals::half;
use crate::routes::leaves::{num, DayHalf, PersonRef, TypeRef};
use axum::extract::{Query, State};
use axum::Json;
use chrono::{Datelike, Duration, Months, NaiveDate};
use leavo_core::visibility::{can_see_details, Subject, Viewer};
use leavo_db::tx::begin_org_tx;
use leavo_db::{approvals_repo, away_repo, leaves_repo};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

#[derive(Serialize, ToSchema)]
pub struct AwayItem {
    pub user: PersonRef,
    pub from: NaiveDate,
    pub to: NaiveDate,
    pub days: f64,
    /// Only for the person's eligible approvers and admins (FR-058).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub leave_type: Option<TypeRef>,
    /// Only for the person's eligible approvers and admins (FR-058).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

#[derive(Serialize, ToSchema)]
pub struct AwayList {
    /// People away today (full or half day) within the scope.
    pub count_today: i64,
    /// Approved leave overlapping the range, sorted by start date.
    pub items: Vec<AwayItem>,
}

#[derive(Deserialize)]
pub struct AwayQuery {
    pub range: Option<String>,
    pub scope: Option<String>,
}

#[derive(Deserialize)]
pub struct CalendarQuery {
    pub month: Option<String>,
    pub scope: Option<String>,
}

#[derive(Serialize, ToSchema)]
pub struct CalendarPerson {
    pub user: PersonRef,
    /// `null` for the full day.
    pub half: Option<DayHalf>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub leave_type: Option<TypeRef>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

#[derive(Serialize, ToSchema)]
pub struct CalendarDay {
    pub date: NaiveDate,
    pub people: Vec<CalendarPerson>,
}

#[derive(Serialize, ToSchema)]
pub struct TeamCalendar {
    /// `YYYY-MM`
    pub month: String,
    /// Days with someone away, in order; other days are omitted.
    pub days: Vec<CalendarDay>,
}

/// `scope=org` (default) or `team` → the viewer whose team filters the list.
fn team_of(scope: Option<&str>, viewer: Uuid) -> ApiResult<Option<Uuid>> {
    match scope {
        None | Some("org") => Ok(None),
        Some("team") => Ok(Some(viewer)),
        Some(_) => Err(field_error("scope")),
    }
}

/// `today`, the Monday–Sunday week containing it, or its calendar month.
fn range_bounds(range: Option<&str>, today: NaiveDate) -> ApiResult<(NaiveDate, NaiveDate)> {
    match range {
        None | Some("today") => Ok((today, today)),
        Some("week") => {
            let monday = today - Duration::days(i64::from(today.weekday().num_days_from_monday()));
            Ok((monday, monday + Duration::days(6)))
        }
        Some("month") => Ok(month_bounds(today.with_day(1).unwrap_or(today))),
        Some(_) => Err(field_error("range")),
    }
}

fn month_bounds(first: NaiveDate) -> (NaiveDate, NaiveDate) {
    let next = first + Months::new(1);
    (first, next - Duration::days(1))
}

fn details(
    viewer: &Viewer<'_>,
    user: Uuid,
    group: Option<Uuid>,
    leave_type: TypeRef,
    reason: String,
) -> (Option<TypeRef>, Option<String>) {
    if can_see_details(viewer, &Subject { user, group }) {
        (Some(leave_type), Some(reason))
    } else {
        (None, None)
    }
}

#[utoipa::path(get, path = "/api/v1/away", tag = "me",
    params(
        ("range" = Option<String>, Query, description = "`today` (default), `week` (Monday–Sunday) or `month`, in the org's time zone"),
        ("scope" = Option<String>, Query, description = "`org` (default) or `team`: the viewer's approver group, the groups they approve, or their department"),
    ),
    responses(
        (status = 200, description = "Approved leave overlapping the range; `leave_type` and `reason` only for eligible approvers and admins (FR-058)", body = AwayList),
        (status = 401, description = "Not signed in", body = crate::openapi::ErrorBody),
        (status = 422, description = "VALIDATION: `details.field` is `range` or `scope`", body = crate::openapi::ErrorBody),
    ))]
pub async fn away(
    State(state): State<AppState>,
    user: CurrentUser,
    Query(q): Query<AwayQuery>,
) -> ApiResult<Json<AwayList>> {
    let team = team_of(q.scope.as_deref(), user.id)?;
    let mut tx = begin_org_tx(&state.db, user.org_id).await?;
    let cal = leaves_repo::org_calendar(&mut tx, user.org_id, state.clock.now()).await?;
    let (from, to) = range_bounds(q.range.as_deref(), cal.today)?;
    let groups = approvals_repo::groups_of(&mut tx, user.id).await?;
    let viewer = Viewer {
        id: user.id,
        role: user.role,
        approves: &groups,
    };
    let count_today = away_repo::count_on(&mut tx, team, cal.today).await?;
    let items = away_repo::requests_between(&mut tx, team, from, to)
        .await?
        .into_iter()
        .map(|r| {
            let (leave_type, reason) = details(
                &viewer,
                r.user_id,
                r.group_id,
                TypeRef {
                    id: r.leave_type_id,
                    code: r.code,
                    name: r.name,
                    color: r.color,
                },
                r.reason,
            );
            AwayItem {
                user: PersonRef {
                    id: r.user_id,
                    full_name: r.full_name,
                },
                from: r.from_date,
                to: r.to_date,
                days: num(r.days),
                leave_type,
                reason,
            }
        })
        .collect();
    tx.commit().await?;
    Ok(Json(AwayList { count_today, items }))
}

#[utoipa::path(get, path = "/api/v1/team/calendar", tag = "me",
    params(
        ("month" = Option<String>, Query, description = "`YYYY-MM`; defaults to the current month in the org's time zone"),
        ("scope" = Option<String>, Query, description = "`org` (default) or `team`, as for `/away`"),
    ),
    responses(
        (status = 200, description = "Booked days of approved leave in the month, per day, with the same field rule as `/away`", body = TeamCalendar),
        (status = 401, description = "Not signed in", body = crate::openapi::ErrorBody),
        (status = 422, description = "VALIDATION: `details.field` is `month` or `scope`", body = crate::openapi::ErrorBody),
    ))]
pub async fn team_calendar(
    State(state): State<AppState>,
    user: CurrentUser,
    Query(q): Query<CalendarQuery>,
) -> ApiResult<Json<TeamCalendar>> {
    let team = team_of(q.scope.as_deref(), user.id)?;
    let mut tx = begin_org_tx(&state.db, user.org_id).await?;
    let cal = leaves_repo::org_calendar(&mut tx, user.org_id, state.clock.now()).await?;
    let first = match q.month.as_deref() {
        None => cal.today.with_day(1).unwrap_or(cal.today),
        Some(m) if m.len() == 7 => NaiveDate::parse_from_str(&format!("{m}-01"), "%Y-%m-%d")
            .ok()
            .filter(|d| (2000..=2200).contains(&d.year()))
            .ok_or_else(|| field_error("month"))?,
        Some(_) => return Err(field_error("month")),
    };
    let (from, to) = month_bounds(first);
    let groups = approvals_repo::groups_of(&mut tx, user.id).await?;
    let viewer = Viewer {
        id: user.id,
        role: user.role,
        approves: &groups,
    };
    let mut days: Vec<CalendarDay> = Vec::new();
    for d in away_repo::days_between(&mut tx, team, from, to).await? {
        let (leave_type, reason) = details(
            &viewer,
            d.user_id,
            d.group_id,
            TypeRef {
                id: d.leave_type_id,
                code: d.code,
                name: d.name,
                color: d.color,
            },
            d.reason,
        );
        let person = CalendarPerson {
            user: PersonRef {
                id: d.user_id,
                full_name: d.full_name,
            },
            half: half(&d.half),
            leave_type,
            reason,
        };
        match days.last_mut() {
            Some(last) if last.date == d.day => last.people.push(person),
            _ => days.push(CalendarDay {
                date: d.day,
                people: vec![person],
            }),
        }
    }
    tx.commit().await?;
    Ok(Json(TeamCalendar {
        month: first.format("%Y-%m").to_string(),
        days,
    }))
}
