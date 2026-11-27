//! Test data factories. They write with the owner pool (bypassing RLS) for speed and clarity.
use super::TestApp;
use chrono::NaiveDate;
use leavo_core::tokens::new_session_token;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct Org {
    pub id: Uuid,
    pub domain: String,
}

#[derive(Debug, Clone)]
pub struct User {
    pub id: Uuid,
    pub org_id: Uuid,
    pub email: String,
}

pub async fn create_org(app: &TestApp, name: &str, domain: &str) -> Org {
    let id = Uuid::now_v7();
    sqlx::query("INSERT INTO organizations (id, name) VALUES ($1, $2)")
        .bind(id)
        .bind(name)
        .execute(&app.db.owner)
        .await
        .unwrap();
    sqlx::query("INSERT INTO org_domains (id, org_id, domain) VALUES ($1, $2, $3)")
        .bind(Uuid::now_v7())
        .bind(id)
        .bind(domain)
        .execute(&app.db.owner)
        .await
        .unwrap();
    Org {
        id,
        domain: domain.to_string(),
    }
}

pub struct NewUser<'a> {
    pub email: &'a str,
    pub role: &'a str,
    pub status: &'a str,
    pub group: Option<Uuid>,
    pub date_of_joining: Option<NaiveDate>,
}

impl<'a> NewUser<'a> {
    pub fn employee(email: &'a str) -> Self {
        Self {
            email,
            role: "employee",
            status: "active",
            group: None,
            date_of_joining: None,
        }
    }
    pub fn role(mut self, role: &'a str) -> Self {
        self.role = role;
        self
    }
    pub fn status(mut self, status: &'a str) -> Self {
        self.status = status;
        self
    }
    pub fn group(mut self, group: Uuid) -> Self {
        self.group = Some(group);
        self
    }
    pub fn joined(mut self, date: NaiveDate) -> Self {
        self.date_of_joining = Some(date);
        self
    }
}

/// An approver group with the given members (approvers or admins).
pub async fn create_group(app: &TestApp, org: &Org, name: &str, members: &[&User]) -> Uuid {
    let id = Uuid::now_v7();
    sqlx::query("INSERT INTO approver_groups (id, org_id, name) VALUES ($1, $2, $3)")
        .bind(id)
        .bind(org.id)
        .bind(name)
        .execute(&app.db.owner)
        .await
        .unwrap();
    for m in members {
        sqlx::query(
            "INSERT INTO approver_group_members (group_id, user_id, org_id) VALUES ($1, $2, $3)",
        )
        .bind(id)
        .bind(m.id)
        .bind(org.id)
        .execute(&app.db.owner)
        .await
        .unwrap();
    }
    id
}

/// Put an existing user in an approver group.
pub async fn set_group(app: &TestApp, user: &User, group: Option<Uuid>) {
    sqlx::query("UPDATE users SET approver_group_id = $2 WHERE id = $1")
        .bind(user.id)
        .bind(group)
        .execute(&app.db.owner)
        .await
        .unwrap();
}

pub async fn add_holiday(app: &TestApp, org: &Org, date: NaiveDate, name: &str, kind: &str) {
    sqlx::query(
        "INSERT INTO holidays (id, org_id, date, name, kind) VALUES ($1, $2, $3, $4, $5::holiday_kind)",
    )
    .bind(Uuid::now_v7())
    .bind(org.id)
    .bind(date)
    .bind(name)
    .bind(kind)
    .execute(&app.db.owner)
    .await
    .unwrap();
}

/// Set leave-type rule columns directly, e.g. `("attachment_required_above_days", "2")`.
pub async fn set_type_rule(app: &TestApp, leave_type: Uuid, column: &str, value: &str) {
    sqlx::query(&format!(
        "UPDATE leave_types SET {column} = $2::text::{} WHERE id = $1",
        match column {
            "allow_half_day" | "allow_negative" | "active" => "boolean",
            "min_notice_days" | "max_backdate_days" => "int",
            _ => "numeric",
        }
    ))
    .bind(leave_type)
    .bind(value)
    .execute(&app.db.owner)
    .await
    .unwrap();
}

pub async fn create_user(app: &TestApp, org: &Org, u: NewUser<'_>) -> User {
    let id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO users (id, org_id, email, full_name, role, status, approver_group_id, date_of_joining)
         VALUES ($1, $2, $3, $4, $5::user_role, $6::user_status, $7, $8)",
    )
    .bind(id).bind(org.id).bind(u.email).bind(u.email.split('@').next().unwrap())
    .bind(u.role).bind(u.status).bind(u.group).bind(u.date_of_joining)
    .execute(&app.db.owner).await.unwrap();
    User {
        id,
        org_id: org.id,
        email: u.email.to_string(),
    }
}

/// Insert a session at the app's current clock time; returns the raw cookie token.
pub async fn create_session(app: &TestApp, user: &User, keep_signed_in: bool) -> String {
    use leavo_core::clock::Clock;
    let (token, hash) = new_session_token();
    leavo_db::sessions::create(
        &app.db.app,
        &leavo_db::sessions::NewSession {
            user_id: user.id,
            org_id: user.org_id,
            token_hash: hash.to_vec(),
            keep_signed_in,
            user_agent: Some("test".into()),
            ip: None,
        },
        app.clock.now(),
    )
    .await
    .unwrap();
    token
}

pub struct NewLeaveType<'a> {
    pub code: &'a str,
    pub quota: Option<rust_decimal::Decimal>,
    pub availability: &'a str,
    pub active: bool,
}

impl<'a> NewLeaveType<'a> {
    pub fn all(code: &'a str, quota: rust_decimal::Decimal) -> Self {
        Self {
            code,
            quota: Some(quota),
            availability: "all",
            active: true,
        }
    }
    pub fn comp_off() -> Self {
        Self {
            code: "CO",
            quota: None,
            availability: "all",
            active: true,
        }
    }
}

pub async fn create_leave_type(app: &TestApp, org: &Org, t: NewLeaveType<'_>) -> Uuid {
    let id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO leave_types (id, org_id, code, name, color, yearly_quota, is_comp_off, availability, active)
         VALUES ($1, $2, $3, $3, '#3B82F6', $4, $5, $6::leave_availability, $7)",
    )
    .bind(id).bind(org.id).bind(t.code).bind(t.quota).bind(t.quota.is_none())
    .bind(t.availability).bind(t.active)
    .execute(&app.db.owner).await.unwrap();
    id
}

impl<'a> NewLeaveType<'a> {
    pub fn restricted(code: &'a str, quota: rust_decimal::Decimal) -> Self {
        Self {
            code,
            quota: Some(quota),
            availability: "restricted",
            active: true,
        }
    }
    pub fn inactive(mut self) -> Self {
        self.active = false;
        self
    }
}

/// `(reason, delta, leave_year, ref_kind)` of every ledger row for a user and type, oldest first.
pub async fn ledger_rows(
    app: &TestApp,
    user: Uuid,
    leave_type: Uuid,
) -> Vec<(String, rust_decimal::Decimal, i32, Option<String>)> {
    sqlx::query_as(
        "SELECT reason::text, delta, leave_year, ref_kind FROM balance_ledger
         WHERE user_id = $1 AND leave_type_id = $2 ORDER BY created_at, id",
    )
    .bind(user)
    .bind(leave_type)
    .fetch_all(&app.db.owner)
    .await
    .unwrap()
}

/// Write a ledger row directly (test setup only).
pub async fn add_ledger_row(
    app: &TestApp,
    user: &User,
    leave_type: Uuid,
    leave_year: i32,
    delta: rust_decimal::Decimal,
    reason: &str,
) {
    sqlx::query(
        "INSERT INTO balance_ledger (id, org_id, user_id, leave_type_id, leave_year, delta, reason, note)
         VALUES ($1, $2, $3, $4, $5, $6, $7::ledger_reason, 'test')",
    )
    .bind(Uuid::now_v7())
    .bind(user.org_id)
    .bind(user.id)
    .bind(leave_type)
    .bind(leave_year)
    .bind(delta)
    .bind(reason)
    .execute(&app.db.owner)
    .await
    .unwrap();
}

pub async fn set_department(app: &TestApp, user: &User, department: &str) {
    sqlx::query("UPDATE users SET department = $2 WHERE id = $1")
        .bind(user.id)
        .bind(department)
        .execute(&app.db.owner)
        .await
        .unwrap();
}

pub async fn set_full_name(app: &TestApp, user: &User, name: &str) {
    sqlx::query("UPDATE users SET full_name = $2 WHERE id = $1")
        .bind(user.id)
        .bind(name)
        .execute(&app.db.owner)
        .await
        .unwrap();
}
